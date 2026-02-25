//! # Structured BF16 Distance — Hierarchical Causal Comparator
//!
//! Computes distance between two 16,384-bit containers (1024 × BF16 elements)
//! by decomposing each BF16 value into its three causal layers:
//!
//! ```text
//! BF16 layout:  [ sign (1b) | exponent (8b) | mantissa (7b) ]
//!                 bit 15       bits 14..7       bits 6..0
//! ```
//!
//! # Hierarchy
//!
//! The three layers encode qualitatively different signals:
//!
//! | Layer    | Meaning              | Operation       | Pearl Rung |
//! |----------|----------------------|-----------------|------------|
//! | Sign     | Causal direction     | XOR (flip/same) | Rung 3     |
//! | Exponent | Causal magnitude     | abs_diff (scale) | Rung 2    |
//! | Mantissa | Correlational texture | popcount (noise) | Rung 1   |
//!
//! # Gating Rule
//!
//! Mantissa is only meaningful within a magnitude band. If sign differs
//! or exponent diverges beyond `EXP_GATE`, mantissa distance is zero —
//! the representations are already causally incomparable at that element.
//!
//! # Integration
//!
//! - **L0 (Belichtungsmesser)**: Flat Hamming on sampled words. Cheap probe.
//! - **L1 (Early-exit)**: Flat Hamming with threshold. Cheap reject.
//! - **L2 (Tail ranking)**: `structured_bf16_distance`. Causal ordering.
//!
//! The cascade preserves speed (L0/L1) and injects causality only at
//! the ranking boundary where it matters.

use serde::{Deserialize, Serialize};

// ============================================================================
// Constants
// ============================================================================

/// Weight for sign disagreement. A single sign flip is a qualitative
/// conflict — "this dimension points the other way." Dominates distance.
pub const W_SIGN: u32 = 64;

/// Weight per exponent step. Exponent is decoded as integer magnitude,
/// not bit pattern. `abs(ea - eb)` gives true scale divergence.
/// Max contribution per element: 4 × 255 = 1020.
pub const W_EXP: u32 = 4;

/// Weight per mantissa bit flip. Only charged when sign matches AND
/// exponent is within `EXP_GATE`. Max contribution per element: 1 × 7 = 7.
pub const W_MANT: u32 = 1;

/// Maximum exponent delta for mantissa to be considered meaningful.
/// If exponents differ by more than this, the values are in different
/// magnitude bands and mantissa comparison is noise.
pub const EXP_GATE: u32 = 2;

/// Number of BF16 elements in one 16,384-bit container (16384 / 16).
pub const ELEMENTS_PER_CONTAINER: usize = 1024;

// BF16 field masks (applied to u16)
const SIGN_MASK: u16 = 0x8000; // bit 15
const EXP_MASK: u16 = 0x7F80;  // bits 14..7
const MANT_MASK: u16 = 0x007F; // bits 6..0

// ============================================================================
// Pre-bias encoding — the zero-exponent fix
// ============================================================================
//
// IEEE 754 zero (0x0000) has exponent 0. The value 0.05 has exponent 122.
// That's a 122-step gap in `abs_diff(exp_a, exp_b)` for what is emotionally
// a tiny difference. This destroys ranking for any vector space with zeros.
//
// Fix: shift all values into a narrow exponent band before BF16 encoding.
// The bias maps [-1.0, 1.0] → [1.0, 3.0], keeping all exponents in the
// range 127..129 (3 steps max). The sign bit is sacrificed — direction is
// encoded by whether the biased value is above or below the midpoint (2.0).
//
// For the qualia space [-0.4, 1.0]:
//   -0.4 → 1.6 (exp=127, above 1.0)
//    0.0 → 2.0 (exp=128, midpoint)
//    0.5 → 2.5 (exp=128)
//    1.0 → 3.0 (exp=128)
//
// This keeps all exponent deltas ≤ 1 for values in [-1.0, 1.0], making
// mantissa the primary discriminator — which is correct for dense vectors
// where most values are in similar magnitude bands.

/// Bias offset: shifts values so zero maps to 2.0 in BF16.
pub const BIAS_OFFSET: f32 = 2.0;

/// Encode an f32 qualia value to biased BF16.
/// Input range: [-1.0, 1.0]. Output: BF16 in [1.0, 3.0].
#[inline(always)]
pub fn qualia_to_bf16(val: f32) -> u16 {
    let biased = val + BIAS_OFFSET;
    // Truncate f32 → bf16 (drop lower 16 mantissa bits)
    (biased.to_bits() >> 16) as u16
}

/// Decode biased BF16 back to f32 qualia value.
#[inline(always)]
pub fn bf16_to_qualia(bf16: u16) -> f32 {
    let biased = f32::from_bits((bf16 as u32) << 16);
    biased - BIAS_OFFSET
}

/// Encode a full qualia vector (any length) to biased BF16.
pub fn qualia_vec_to_bf16(vals: &[f32]) -> Vec<u16> {
    vals.iter().map(|&v| qualia_to_bf16(v)).collect()
}

/// Decode a biased BF16 vector back to f32.
pub fn bf16_vec_to_qualia(bf16s: &[u16]) -> Vec<f32> {
    bf16s.iter().map(|&b| bf16_to_qualia(b)).collect()
}

// ============================================================================
// Layer Counts — the "free" awareness output
// ============================================================================

/// Per-layer distance counters accumulated during structured distance.
///
/// These map directly to `AwarenessTensor` layer values without placeholders:
/// - `sign_flips` / `total_elements` → sign agreement ratio
/// - `exp_delta_sum` / `max_exp_delta_sum` → exponent agreement ratio
/// - `mant_bit_flips` / `max_mant_bits` → mantissa agreement ratio (gated)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerCounts {
    /// Number of elements where sign bits disagree (0..1024).
    pub sign_flips: u32,
    /// Sum of |exp_a - exp_b| across all elements (0..261120, i.e. 1024×255).
    pub exp_delta_sum: u32,
    /// Number of mantissa bits that differ, counted ONLY for elements
    /// where sign matches AND exponent delta ≤ EXP_GATE (0..7168, i.e. 1024×7).
    pub mant_bit_flips: u32,
    /// Number of elements where mantissa was actually compared (gated count).
    pub mant_elements_compared: u32,
    /// Total elements compared.
    pub total_elements: u32,
}

impl LayerCounts {
    /// Sign agreement ratio in [0.0, 1.0]. 1.0 = all signs match.
    pub fn sign_agreement(&self) -> f32 {
        if self.total_elements == 0 { return 0.0; }
        1.0 - (self.sign_flips as f32 / self.total_elements as f32)
    }

    /// Exponent agreement ratio in [0.0, 1.0]. 1.0 = all exponents identical.
    /// Normalized by max possible delta (255 per element).
    pub fn exp_agreement(&self) -> f32 {
        if self.total_elements == 0 { return 0.0; }
        let max_sum = self.total_elements as f32 * 255.0;
        1.0 - (self.exp_delta_sum as f32 / max_sum)
    }

    /// Mantissa agreement ratio in [0.0, 1.0]. Only counts gated elements.
    /// 1.0 = all compared mantissa bits identical (or none compared).
    pub fn mant_agreement(&self) -> f32 {
        if self.mant_elements_compared == 0 { return 0.0; }
        let max_bits = self.mant_elements_compared as f32 * 7.0;
        1.0 - (self.mant_bit_flips as f32 / max_bits)
    }

    /// Fraction of elements that passed the gate (sign match + exp close).
    /// Low ratio means the containers are causally divergent at most positions.
    pub fn gate_pass_ratio(&self) -> f32 {
        if self.total_elements == 0 { return 0.0; }
        self.mant_elements_compared as f32 / self.total_elements as f32
    }
}

// ============================================================================
// Core distance: per-element
// ============================================================================

/// Hierarchical causal distance for a single BF16 element pair.
///
/// Returns (score, sign_flipped, exp_delta, mant_bits_if_gated).
#[inline(always)]
fn bf16_element_distance(a: u16, b: u16) -> (u32, bool, u8, Option<u32>) {
    let sa = (a & SIGN_MASK) >> 15;
    let sb = (b & SIGN_MASK) >> 15;
    let ea = ((a & EXP_MASK) >> 7) as u8;
    let eb = ((b & EXP_MASK) >> 7) as u8;

    let sign_diff = sa ^ sb; // 0 or 1
    let exp_delta = ea.abs_diff(eb);

    let mut score = W_SIGN * sign_diff as u32 + W_EXP * exp_delta as u32;

    // Mantissa is only meaningful in same magnitude neighborhood
    let mant_bits = if sign_diff == 0 && (exp_delta as u32) <= EXP_GATE {
        let ma = a & MANT_MASK;
        let mb = b & MANT_MASK;
        let bits = (ma ^ mb).count_ones();
        score += W_MANT * bits;
        Some(bits)
    } else {
        None
    };

    (score, sign_diff != 0, exp_delta, mant_bits)
}

// ============================================================================
// Container-level distance
// ============================================================================

/// Result of comparing two 16,384-bit containers via structured BF16 distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bf16Distance {
    /// Weighted hierarchical distance score.
    /// Range: 0..=1_091_584 (1024 × max_per_element where max = 64 + 4×255 + 7 = 1091)
    pub score: u32,
    /// Per-layer breakdown for awareness tensor population.
    pub layers: LayerCounts,
}

impl Bf16Distance {
    /// Normalized distance in [0.0, 1.0].
    pub fn normalized(&self) -> f32 {
        // Max score per element: W_SIGN×1 + W_EXP×255 + W_MANT×7 = 64 + 1020 + 7 = 1091
        // But mantissa is gated, so theoretical max is W_SIGN + W_EXP×255 = 1084 when
        // sign differs (no mantissa). Use 1084 as normalizer for worst case.
        const MAX_PER_ELEMENT: f32 = (W_SIGN + W_EXP * 255) as f32;
        const MAX_TOTAL: f32 = MAX_PER_ELEMENT * ELEMENTS_PER_CONTAINER as f32;
        self.score as f32 / MAX_TOTAL
    }

    /// Similarity in [0.0, 1.0] — inverse of normalized distance.
    pub fn similarity(&self) -> f32 {
        1.0 - self.normalized()
    }
}

/// Compute structured BF16 distance between two containers.
///
/// Each container is 256 × u64 = 16,384 bits = 1024 × BF16 elements.
/// The u64 words are interpreted as pairs of u16 BF16 values in little-endian order.
///
/// # Panics
///
/// Panics if either slice length is not 256.
pub fn structured_bf16_distance(a: &[u64], b: &[u64]) -> Bf16Distance {
    assert_eq!(a.len(), 256, "container must be 256 × u64");
    assert_eq!(b.len(), 256, "container must be 256 × u64");

    let mut score = 0u32;
    let mut layers = LayerCounts {
        total_elements: ELEMENTS_PER_CONTAINER as u32,
        ..Default::default()
    };

    // Process 4 BF16 elements per u64 word
    for i in 0..256 {
        let wa = a[i];
        let wb = b[i];

        // Extract 4 × u16 from each u64
        let elems_a = [
            (wa & 0xFFFF) as u16,
            ((wa >> 16) & 0xFFFF) as u16,
            ((wa >> 32) & 0xFFFF) as u16,
            ((wa >> 48) & 0xFFFF) as u16,
        ];
        let elems_b = [
            (wb & 0xFFFF) as u16,
            ((wb >> 16) & 0xFFFF) as u16,
            ((wb >> 32) & 0xFFFF) as u16,
            ((wb >> 48) & 0xFFFF) as u16,
        ];

        for j in 0..4 {
            let (s, sign_flipped, exp_delta, mant_bits) =
                bf16_element_distance(elems_a[j], elems_b[j]);

            score += s;

            if sign_flipped {
                layers.sign_flips += 1;
            }
            layers.exp_delta_sum += exp_delta as u32;

            if let Some(bits) = mant_bits {
                layers.mant_bit_flips += bits;
                layers.mant_elements_compared += 1;
            }
        }
    }

    Bf16Distance { score, layers }
}

/// Compute structured BF16 distance from raw u16 slices.
///
/// Convenience function for when data is already in u16 form.
///
/// # Panics
///
/// Panics if either slice length is not 1024.
pub fn structured_bf16_distance_u16(a: &[u16], b: &[u16]) -> Bf16Distance {
    assert_eq!(a.len(), ELEMENTS_PER_CONTAINER, "must be 1024 BF16 elements");
    assert_eq!(b.len(), ELEMENTS_PER_CONTAINER, "must be 1024 BF16 elements");

    let mut score = 0u32;
    let mut layers = LayerCounts {
        total_elements: ELEMENTS_PER_CONTAINER as u32,
        ..Default::default()
    };

    for i in 0..ELEMENTS_PER_CONTAINER {
        let (s, sign_flipped, exp_delta, mant_bits) =
            bf16_element_distance(a[i], b[i]);

        score += s;

        if sign_flipped {
            layers.sign_flips += 1;
        }
        layers.exp_delta_sum += exp_delta as u32;

        if let Some(bits) = mant_bits {
            layers.mant_bit_flips += bits;
            layers.mant_elements_compared += 1;
        }
    }

    Bf16Distance { score, layers }
}

// ============================================================================
// SPO-level: comparing two edges across Subject, Predicate, Object
// ============================================================================

/// Full SPO comparison result — three `Bf16Distance` values producing
/// a real `AwarenessTensor` with no placeholders.
#[derive(Debug, Clone)]
pub struct SpoDistance {
    pub subject: Bf16Distance,
    pub predicate: Bf16Distance,
    pub object: Bf16Distance,
}

impl SpoDistance {
    /// Total weighted score across all three SPO dimensions.
    pub fn total_score(&self) -> u32 {
        self.subject.score + self.predicate.score + self.object.score
    }

    /// Build a real `AwarenessTensor` from the layer counts.
    ///
    /// Each cell is a genuine agreement ratio computed from actual
    /// BF16 decomposition — not a placeholder.
    pub fn to_awareness_tensor(&self) -> super::awareness::AwarenessTensor {
        super::awareness::AwarenessTensor {
            s_sign: self.subject.layers.sign_agreement(),
            s_exp:  self.subject.layers.exp_agreement(),
            s_mant: self.subject.layers.mant_agreement(),
            p_sign: self.predicate.layers.sign_agreement(),
            p_exp:  self.predicate.layers.exp_agreement(),
            p_mant: self.predicate.layers.mant_agreement(),
            o_sign: self.object.layers.sign_agreement(),
            o_exp:  self.object.layers.exp_agreement(),
            o_mant: self.object.layers.mant_agreement(),
        }
    }
}

/// Compare two edges across all three SPO containers.
///
/// Each parameter is a 256 × u64 container. Returns the full SPO distance
/// with per-layer counts that produce a real `AwarenessTensor`.
pub fn spo_distance(
    s_a: &[u64], s_b: &[u64],
    p_a: &[u64], p_b: &[u64],
    o_a: &[u64], o_b: &[u64],
) -> SpoDistance {
    SpoDistance {
        subject:   structured_bf16_distance(s_a, s_b),
        predicate: structured_bf16_distance(p_a, p_b),
        object:    structured_bf16_distance(o_a, o_b),
    }
}

// ============================================================================
// NIB4 — 4-bit Nibble Encoding (the F:F approach)
// ============================================================================
//
// Each qualia dimension → single hex nibble 0x0..0xF (0..15).
// Distance = Manhattan (sum of abs_diff per dimension).
//
// Why this wins over BF16:
// - BF16 needs 3-step cascade (sign → exp → mant with gating)
// - Nib4 needs 1 step: abs_diff per nibble. Done.
// - 17 dims × 4 bits = 68 bits. Leaves 16,316 bits for graph topology.
// - Minimum separation ≥ 1 step = 1/15 of per-dim range.
// - Per-dim quantization uses all 16 levels (vs 7 mantissa bits in BF16).
//
// Container layout:
// ```text
// [ 68 bits: 17 qualia nibbles ][ 16,316 bits: nodes/edges/NARS/SQL/GQL/... ]
// ```

/// Number of nibble levels (4 bits = 16 levels, 0..15).
pub const NIB4_LEVELS: u8 = 15;

/// Number of qualia dimensions in the standard taxonomy.
pub const QUALIA_DIMS: usize = 17;

/// Bits consumed by qualia nibbles in a container.
pub const QUALIA_BITS: usize = QUALIA_DIMS * 4; // 68

/// Bits remaining for graph topology in a 16,384-bit container.
pub const TOPOLOGY_BITS: usize = 16_384 - QUALIA_BITS; // 16,316

/// Per-dimension quantization bounds.
/// Each dimension has its own [min, max] so all 16 levels are used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nib4Codebook {
    /// (min, max) per dimension. Length = QUALIA_DIMS.
    pub bounds: Vec<(f32, f32)>,
}

impl Nib4Codebook {
    /// Build codebook from a corpus of float vectors.
    /// Each vector must have length == QUALIA_DIMS.
    pub fn from_corpus(vectors: &[&[f32]]) -> Self {
        assert!(!vectors.is_empty(), "corpus must not be empty");
        let ndims = vectors[0].len();
        let mut bounds = Vec::with_capacity(ndims);

        for d in 0..ndims {
            let mut mn = f32::INFINITY;
            let mut mx = f32::NEG_INFINITY;
            for v in vectors {
                let val = v[d];
                if val < mn { mn = val; }
                if val > mx { mx = val; }
            }
            // Tiny epsilon to avoid div-by-zero on constant dimensions
            if (mx - mn).abs() < 1e-9 {
                mx = mn + 1.0;
            }
            bounds.push((mn, mx));
        }

        Self { bounds }
    }

    /// Quantize a single float value for dimension `dim` → 0..15.
    #[inline(always)]
    pub fn encode_dim(&self, dim: usize, val: f32) -> u8 {
        let (mn, mx) = self.bounds[dim];
        let t = (val - mn) / (mx - mn); // 0.0..1.0
        (t * NIB4_LEVELS as f32).round().clamp(0.0, NIB4_LEVELS as f32) as u8
    }

    /// Decode a nibble value back to float for dimension `dim`.
    #[inline(always)]
    pub fn decode_dim(&self, dim: usize, nib: u8) -> f32 {
        let (mn, mx) = self.bounds[dim];
        mn + (nib as f32 / NIB4_LEVELS as f32) * (mx - mn)
    }

    /// Encode a full float vector → nibble vector.
    pub fn encode_vec(&self, vals: &[f32]) -> Vec<u8> {
        vals.iter()
            .enumerate()
            .map(|(d, &v)| self.encode_dim(d, v))
            .collect()
    }

    /// Decode a nibble vector back to floats.
    pub fn decode_vec(&self, nibs: &[u8]) -> Vec<f32> {
        nibs.iter()
            .enumerate()
            .map(|(d, &n)| self.decode_dim(d, n))
            .collect()
    }

    /// Pack nibble vector into a compact u128 (68 bits for 17 dims).
    /// Nibble 0 goes to bits 3..0, nibble 1 to bits 7..4, etc.
    pub fn pack_u128(&self, nibs: &[u8]) -> u128 {
        let mut packed: u128 = 0;
        for (i, &n) in nibs.iter().enumerate() {
            packed |= (n as u128 & 0xF) << (i * 4);
        }
        packed
    }

    /// Unpack u128 back to nibble vector.
    pub fn unpack_u128(&self, packed: u128, ndims: usize) -> Vec<u8> {
        (0..ndims)
            .map(|i| ((packed >> (i * 4)) & 0xF) as u8)
            .collect()
    }
}

/// Manhattan distance between two nibble vectors.
/// Sum of abs_diff per dimension. One operation per dim. That's it.
#[inline]
pub fn nib4_distance(a: &[u8], b: &[u8]) -> u32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(&x, &y)| x.abs_diff(y) as u32)
        .sum()
}

/// Manhattan distance from packed u128 representations.
#[inline]
pub fn nib4_distance_packed(a: u128, b: u128, ndims: usize) -> u32 {
    let mut dist = 0u32;
    for i in 0..ndims {
        let na = ((a >> (i * 4)) & 0xF) as u8;
        let nb = ((b >> (i * 4)) & 0xF) as u8;
        dist += na.abs_diff(nb) as u32;
    }
    dist
}

/// Normalized nibble distance in [0.0, 1.0].
/// Normalized by F × ndims (max possible Manhattan distance).
#[inline]
pub fn nib4_distance_normalized(a: &[u8], b: &[u8]) -> f32 {
    let raw = nib4_distance(a, b);
    let max = NIB4_LEVELS as u32 * a.len() as u32; // F × ndims
    raw as f32 / max as f32
}

/// Format a nibble vector as hex string "8:C:A:7:9:3:B:..."
pub fn nib4_to_hex(nibs: &[u8]) -> String {
    nibs.iter()
        .map(|n| format!("{:X}", n & 0xF))
        .collect::<Vec<_>>()
        .join(":")
}

// ============================================================================
// BF16-aligned packing: 4 nibbles per u16 word
// ============================================================================
//
// Container layout (1024 × u16 = BF16 format):
//
// ```text
// word 0:  [ nib_3 | nib_2 | nib_1 | nib_0 ]   ← 4 qualia dims
// word 1:  [ nib_7 | nib_6 | nib_5 | nib_4 ]   ← 4 qualia dims
// word 2:  [ nib_11| nib_10| nib_9 | nib_8 ]   ← 4 qualia dims
// word 3:  [ nib_15| nib_14| nib_13| nib_12]   ← 4 qualia dims
// word 4:  [ 0000  | 0000  | 0000  | nib_16]   ← 1 qualia dim + 3 spare
// word 5..1023: topology (nodes/edges/NARS/SQL/GQL/DNtree/Btree)
// ```
//
// This aligns exactly with BF16: same u16 word size, same container layout.
// The Hamming hardware sees no difference — just u16 words.

/// Number of u16 words needed for qualia nibbles.
pub const QUALIA_WORDS: usize = (QUALIA_DIMS + 3) / 4; // ceil(17/4) = 5

/// Pack nibble vector into BF16-aligned u16 words.
/// Returns a Vec<u16> of length QUALIA_WORDS (5 for 17 dims).
pub fn nib4_pack_bf16(nibs: &[u8]) -> Vec<u16> {
    let nwords = (nibs.len() + 3) / 4;
    let mut words = vec![0u16; nwords];
    for (i, &n) in nibs.iter().enumerate() {
        let word_idx = i / 4;
        let nib_pos = i % 4;
        words[word_idx] |= ((n & 0xF) as u16) << (nib_pos * 4);
    }
    words
}

/// Unpack BF16-aligned u16 words back to nibble vector.
pub fn nib4_unpack_bf16(words: &[u16], ndims: usize) -> Vec<u8> {
    (0..ndims)
        .map(|i| {
            let word_idx = i / 4;
            let nib_pos = i % 4;
            ((words[word_idx] >> (nib_pos * 4)) & 0xF) as u8
        })
        .collect()
}

/// Manhattan distance directly on BF16-aligned packed u16 words.
/// Operates on the qualia portion only (first QUALIA_WORDS words).
pub fn nib4_distance_bf16_aligned(a: &[u16], b: &[u16], ndims: usize) -> u32 {
    let nwords = (ndims + 3) / 4;
    let mut dist = 0u32;
    for w in 0..nwords {
        let wa = a[w];
        let wb = b[w];
        // Process 4 nibbles per word
        let nibs_in_word = if w == nwords - 1 { ((ndims - 1) % 4) + 1 } else { 4 };
        for p in 0..nibs_in_word {
            let na = ((wa >> (p * 4)) & 0xF) as u8;
            let nb = ((wb >> (p * 4)) & 0xF) as u8;
            dist += na.abs_diff(nb) as u32;
        }
    }
    dist
}

/// SPO nibble distance — compare S, P, O qualia vectors.
#[derive(Debug, Clone)]
pub struct SpoNib4Distance {
    pub subject: u32,
    pub predicate: u32,
    pub object: u32,
}

impl SpoNib4Distance {
    pub fn total(&self) -> u32 {
        self.subject + self.predicate + self.object
    }

    pub fn normalized(&self, ndims: usize) -> f32 {
        let max = 3 * NIB4_LEVELS as u32 * ndims as u32;
        self.total() as f32 / max as f32
    }
}

/// Compare two edges across S, P, O nibble vectors.
pub fn spo_nib4_distance(
    s_a: &[u8], s_b: &[u8],
    p_a: &[u8], p_b: &[u8],
    o_a: &[u8], o_b: &[u8],
) -> SpoNib4Distance {
    SpoNib4Distance {
        subject: nib4_distance(s_a, s_b),
        predicate: nib4_distance(p_a, p_b),
        object: nib4_distance(o_a, o_b),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: pack a single BF16 value from components
    fn bf16(sign: u16, exp: u16, mant: u16) -> u16 {
        (sign << 15) | (exp << 7) | mant
    }

    // Helper: create a 1024-element u16 buffer filled with one value
    fn fill_u16(val: u16) -> Vec<u16> {
        vec![val; ELEMENTS_PER_CONTAINER]
    }

    #[test]
    fn identical_containers_have_zero_distance() {
        let a = fill_u16(bf16(0, 127, 64)); // +1.0ish in BF16
        let d = structured_bf16_distance_u16(&a, &a);
        assert_eq!(d.score, 0);
        assert_eq!(d.layers.sign_flips, 0);
        assert_eq!(d.layers.exp_delta_sum, 0);
        assert_eq!(d.layers.mant_bit_flips, 0);
        assert_eq!(d.layers.mant_elements_compared, 1024);
        assert!((d.layers.sign_agreement() - 1.0).abs() < f32::EPSILON);
        assert!((d.layers.exp_agreement() - 1.0).abs() < f32::EPSILON);
        assert!((d.layers.mant_agreement() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn sign_flip_only_charges_sign_penalty() {
        let a = fill_u16(bf16(0, 127, 64)); // positive
        let b = fill_u16(bf16(1, 127, 64)); // negative, same exp+mant

        let d = structured_bf16_distance_u16(&a, &b);

        // Every element has sign flip
        assert_eq!(d.layers.sign_flips, 1024);
        // Exponent identical
        assert_eq!(d.layers.exp_delta_sum, 0);
        // Mantissa NOT compared (sign differs → gated out)
        assert_eq!(d.layers.mant_elements_compared, 0);
        assert_eq!(d.layers.mant_bit_flips, 0);
        // Score = 1024 × W_SIGN = 1024 × 64 = 65536
        assert_eq!(d.score, 1024 * W_SIGN);
    }

    #[test]
    fn exponent_uses_abs_diff_not_popcount() {
        // Exponent 127 (01111111) vs 128 (10000000):
        // popcount(XOR) = 8 bits differ, but abs_diff = 1
        let a = fill_u16(bf16(0, 127, 0));
        let b = fill_u16(bf16(0, 128, 0));

        let d = structured_bf16_distance_u16(&a, &b);

        assert_eq!(d.layers.sign_flips, 0);
        assert_eq!(d.layers.exp_delta_sum, 1024); // 1024 × abs_diff(1)
        // exp_delta=1 ≤ EXP_GATE=2, so mantissa IS compared (all 0s → 0 flips)
        assert_eq!(d.layers.mant_elements_compared, 1024);
        assert_eq!(d.layers.mant_bit_flips, 0);
        // Score = 1024 × W_EXP × 1 = 1024 × 4 = 4096
        assert_eq!(d.score, 1024 * W_EXP * 1);
    }

    #[test]
    fn large_exponent_gap_gates_mantissa_out() {
        // Exponent 10 vs 200: abs_diff = 190, way beyond EXP_GATE
        let a = fill_u16(bf16(0, 10, 0x7F)); // all mantissa bits set
        let b = fill_u16(bf16(0, 200, 0x00)); // no mantissa bits set

        let d = structured_bf16_distance_u16(&a, &b);

        assert_eq!(d.layers.sign_flips, 0);
        assert_eq!(d.layers.exp_delta_sum, 1024 * 190);
        // Mantissa gated out — exponent gap too large
        assert_eq!(d.layers.mant_elements_compared, 0);
        assert_eq!(d.layers.mant_bit_flips, 0);
        // Score = 1024 × W_EXP × 190 = 1024 × 760 = 778240
        assert_eq!(d.score, 1024 * W_EXP * 190);
    }

    #[test]
    fn mantissa_only_when_close_and_same_sign() {
        // Same sign, exponent differs by 1 (within gate), different mantissa
        let a = fill_u16(bf16(0, 100, 0b1010101)); // mant = 85
        let b = fill_u16(bf16(0, 101, 0b0101010)); // mant = 42

        let d = structured_bf16_distance_u16(&a, &b);

        assert_eq!(d.layers.sign_flips, 0);
        assert_eq!(d.layers.exp_delta_sum, 1024 * 1);
        // Gate passes: sign same, exp_delta=1 ≤ 2
        assert_eq!(d.layers.mant_elements_compared, 1024);
        // XOR of mantissa: 1010101 ^ 0101010 = 1111111 → 7 bits
        assert_eq!(d.layers.mant_bit_flips, 1024 * 7);
        // Score = 1024 × (W_EXP×1 + W_MANT×7) = 1024 × (4 + 7) = 11264
        assert_eq!(d.score, 1024 * (W_EXP * 1 + W_MANT * 7));
    }

    #[test]
    fn sign_dominates_exponent_dominates_mantissa() {
        // Verify the hierarchy: one sign flip > many exponent steps > all mantissa bits
        let base = fill_u16(bf16(0, 127, 0));

        // Case A: 1 sign flip, exp/mant identical
        let sign_flip = fill_u16(bf16(1, 127, 0));
        let d_sign = structured_bf16_distance_u16(&base, &sign_flip);

        // Case B: no sign flip, exp differs by 10, mant identical
        let exp_shift = fill_u16(bf16(0, 137, 0));
        let d_exp = structured_bf16_distance_u16(&base, &exp_shift);

        // Case C: no sign flip, exp identical, all 7 mantissa bits differ
        let mant_diff = fill_u16(bf16(0, 127, 0x7F));
        let d_mant = structured_bf16_distance_u16(&base, &mant_diff);

        // Sign (64) > exp×10 (40) > mant×7 (7)
        assert!(d_sign.score > d_exp.score,
            "sign penalty {} should dominate exp penalty {}", d_sign.score, d_exp.score);
        assert!(d_exp.score > d_mant.score,
            "exp penalty {} should dominate mant penalty {}", d_exp.score, d_mant.score);
    }

    #[test]
    fn exponent_127_vs_128_is_less_than_8_vs_248() {
        // This is the critical test: abs_diff correctness vs popcount incorrectness
        // 127→128 flips ALL 8 bits (01111111→10000000) but is scale distance 1
        // 8→248 may flip fewer bits but is scale distance 240
        let base = fill_u16(bf16(0, 127, 0));
        let near = fill_u16(bf16(0, 128, 0));
        let far_a = fill_u16(bf16(0, 8, 0));
        let far_b = fill_u16(bf16(0, 248, 0));

        let d_near = structured_bf16_distance_u16(&base, &near);
        let d_far = structured_bf16_distance_u16(&far_a, &far_b);

        // abs_diff(127,128) = 1 vs abs_diff(8,248) = 240
        assert!(d_near.score < d_far.score,
            "exp 127↔128 (score {}) must be less than 8↔248 (score {})",
            d_near.score, d_far.score);
    }

    #[test]
    fn u64_container_matches_u16_container() {
        // Verify that the u64 path produces the same result as the u16 path
        let a_u16: Vec<u16> = (0..1024).map(|i| bf16(i % 2, (i % 256) as u16, (i % 128) as u16)).collect();
        let b_u16: Vec<u16> = (0..1024).map(|i| bf16((i + 1) % 2, ((i + 10) % 256) as u16, ((i + 5) % 128) as u16)).collect();

        // Pack u16 into u64 (little-endian: 4 u16 per u64)
        let a_u64: Vec<u64> = a_u16.chunks(4).map(|c| {
            c[0] as u64
                | ((c[1] as u64) << 16)
                | ((c[2] as u64) << 32)
                | ((c[3] as u64) << 48)
        }).collect();
        let b_u64: Vec<u64> = b_u16.chunks(4).map(|c| {
            c[0] as u64
                | ((c[1] as u64) << 16)
                | ((c[2] as u64) << 32)
                | ((c[3] as u64) << 48)
        }).collect();

        let d_u16 = structured_bf16_distance_u16(&a_u16, &b_u16);
        let d_u64 = structured_bf16_distance(&a_u64, &b_u64);

        assert_eq!(d_u16.score, d_u64.score);
        assert_eq!(d_u16.layers, d_u64.layers);
    }

    #[test]
    fn layer_counts_produce_valid_agreement_ratios() {
        // Mixed scenario: half elements sign-flipped, half identical
        let mut a = vec![0u16; 1024];
        let mut b = vec![0u16; 1024];

        // First 512: identical (sign=0, exp=100, mant=0)
        for i in 0..512 {
            a[i] = bf16(0, 100, 0);
            b[i] = bf16(0, 100, 0);
        }
        // Next 512: sign flipped
        for i in 512..1024 {
            a[i] = bf16(0, 100, 0);
            b[i] = bf16(1, 100, 0);
        }

        let d = structured_bf16_distance_u16(&a, &b);

        let sign_agr = d.layers.sign_agreement();
        assert!((sign_agr - 0.5).abs() < f32::EPSILON,
            "sign agreement should be 0.5, got {}", sign_agr);

        let exp_agr = d.layers.exp_agreement();
        assert!((exp_agr - 1.0).abs() < f32::EPSILON,
            "exp agreement should be 1.0, got {}", exp_agr);

        // Only 512 elements passed the gate (the identical ones)
        assert_eq!(d.layers.mant_elements_compared, 512);
        assert!((d.layers.gate_pass_ratio() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn spo_distance_produces_awareness_tensor() {
        let s = vec![0u64; 256]; // all zeros
        let p = vec![0u64; 256];
        let o = vec![0u64; 256];

        let dist = spo_distance(&s, &s, &p, &p, &o, &o);
        let tensor = dist.to_awareness_tensor();

        // Identical containers → perfect agreement everywhere
        assert_eq!(tensor.awareness_state(), super::super::awareness::AwarenessState::Crystallized);
        assert!((tensor.total_agreement() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn normalized_distance_in_unit_range() {
        // Worst case: all elements have sign flip + max exponent gap
        let a = fill_u16(bf16(0, 0, 0));
        let b = fill_u16(bf16(1, 255, 0)); // sign flip + exp 255 vs 0

        let d = structured_bf16_distance_u16(&a, &b);

        assert!(d.normalized() > 0.0);
        assert!(d.normalized() <= 1.0);
        assert!(d.similarity() >= 0.0);
        assert!(d.similarity() < 1.0);
    }

    // ====================================================================
    // Nib4 tests
    // ====================================================================

    #[test]
    fn nib4_identical_zero_distance() {
        let a = vec![7u8; 17];
        assert_eq!(nib4_distance(&a, &a), 0);
    }

    #[test]
    fn nib4_max_distance_is_ff() {
        // 17 dims × 15 max abs_diff = 255 = 0xFF
        let a = vec![0u8; 17];
        let b = vec![15u8; 17];
        assert_eq!(nib4_distance(&a, &b), 255); // 0xFF
    }

    #[test]
    fn nib4_manhattan_abs_diff() {
        let a = vec![3, 10, 7, 0, 15, 5, 8, 12, 1, 14, 6, 9, 2, 11, 4, 13, 7];
        let b = vec![5, 8, 7, 3, 12, 5, 10, 9, 4, 11, 6, 6, 5, 8, 7, 10, 7];
        let expected: u32 = a.iter().zip(&b).map(|(x, y): (&u8, &u8)| x.abs_diff(*y) as u32).sum();
        assert_eq!(nib4_distance(&a, &b), expected);
    }

    #[test]
    fn nib4_packed_matches_unpacked() {
        let codebook = Nib4Codebook {
            bounds: vec![(0.0, 1.0); 17],
        };
        let a = vec![3, 10, 7, 0, 15, 5, 8, 12, 1, 14, 6, 9, 2, 11, 4, 13, 7];
        let b = vec![5, 8, 7, 3, 12, 5, 10, 9, 4, 11, 6, 6, 5, 8, 7, 10, 7];
        let pa = codebook.pack_u128(&a);
        let pb = codebook.pack_u128(&b);
        assert_eq!(nib4_distance(&a, &b), nib4_distance_packed(pa, pb, 17));
    }

    #[test]
    fn nib4_codebook_roundtrip() {
        let codebook = Nib4Codebook {
            bounds: vec![(-0.4, 1.0); 17],
        };
        for val in [-0.4f32, -0.2, 0.0, 0.25, 0.5, 0.75, 1.0] {
            let nib = codebook.encode_dim(0, val);
            let decoded = codebook.decode_dim(0, nib);
            assert!((val - decoded).abs() < 0.05,
                "roundtrip {val} → {nib} → {decoded}, err={}", (val - decoded).abs());
        }
    }

    #[test]
    fn nib4_hex_format() {
        let nibs = vec![0xA, 0x5, 0xF, 0x0, 0x7];
        assert_eq!(nib4_to_hex(&nibs), "A:5:F:0:7");
    }

    #[test]
    fn nib4_normalized_bounds() {
        let a = vec![0u8; 17];
        let b = vec![15u8; 17];
        let norm = nib4_distance_normalized(&a, &b);
        assert!((norm - 1.0).abs() < f32::EPSILON, "max distance should normalize to 1.0");

        let norm_zero = nib4_distance_normalized(&a, &a);
        assert!((norm_zero).abs() < f32::EPSILON, "identical should normalize to 0.0");
    }

    #[test]
    fn nib4_17_dims_times_f_equals_ff() {
        // The elegant property: 17 × F = FF (17 × 15 = 255)
        assert_eq!(QUALIA_DIMS as u32 * NIB4_LEVELS as u32, 0xFF);
    }

    #[test]
    fn nib4_bf16_aligned_packing() {
        let nibs = vec![0xA, 0x5, 0xF, 0x0, 0x7, 0x3, 0xB, 0x8,
                        0x1, 0xE, 0x6, 0x9, 0x2, 0xD, 0x4, 0xC, 0x7];
        assert_eq!(nibs.len(), 17); // QUALIA_DIMS

        let packed = nib4_pack_bf16(&nibs);
        assert_eq!(packed.len(), 5); // QUALIA_WORDS = ceil(17/4) = 5

        // First word: nibbles 0-3 → 0xA, 0x5, 0xF, 0x0 → 0x0F5A
        assert_eq!(packed[0], 0x0F5A);

        // Roundtrip
        let unpacked = nib4_unpack_bf16(&packed, 17);
        assert_eq!(unpacked, nibs);
    }

    #[test]
    fn nib4_bf16_aligned_distance_matches_unpacked() {
        let a = vec![0xA, 0x5, 0xF, 0x0, 0x7, 0x3, 0xB, 0x8,
                     0x1, 0xE, 0x6, 0x9, 0x2, 0xD, 0x4, 0xC, 0x7];
        let b = vec![0x3, 0x8, 0xC, 0x2, 0x5, 0x6, 0x9, 0xA,
                     0x4, 0xB, 0x3, 0x6, 0x5, 0xA, 0x7, 0x9, 0x4];

        let d_unpacked = nib4_distance(&a, &b);

        let pa = nib4_pack_bf16(&a);
        let pb = nib4_pack_bf16(&b);
        let d_packed = nib4_distance_bf16_aligned(&pa, &pb, 17);

        assert_eq!(d_unpacked, d_packed,
            "BF16-aligned distance must match unpacked distance");
    }

    #[test]
    fn nib4_qualia_fits_in_5_bf16_words() {
        // 17 dims × 4 bits = 68 bits → 5 × u16 (80 bits, 12 spare)
        assert_eq!(QUALIA_WORDS, 5);
        // That leaves 1019 words for topology
        assert_eq!(ELEMENTS_PER_CONTAINER - QUALIA_WORDS, 1019);
    }
}
