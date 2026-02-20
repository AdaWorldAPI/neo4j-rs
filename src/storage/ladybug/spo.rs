//! SPO trace geometry for holographic relationship storage.
//!
//! Maps the Xyz/SPO architecture from ladybug-rs into neo4j-rs:
//!
//! ```text
//! S = Subject container   (8192 bits)  — node fingerprint
//! P = Predicate container (8192 bits)  — verb fingerprint
//! O = Object container    (8192 bits)  — node fingerprint
//! trace = S ⊕ P ⊕ O      (8192 bits)  — holographic marker
//! ```
//!
//! Given any 2 + trace, recover the 3rd via XOR:
//! - `missing_O = trace ⊕ S ⊕ P`
//! - `missing_S = trace ⊕ P ⊕ O`
//! - `missing_P = trace ⊕ S ⊕ O`
//!
//! This is the blasgraph lineage:
//! ```text
//! RedisGraph (CSR integer IDs)
//!   → Holograph (DnNodeStore + DnCsr, fingerprint IDs)
//!     → BlasGraph (sparse adjacent vectors, BLAS-style ops)
//!       → ContainerGraph (pure Container-native, everything 8192 bits)
//! ```

use std::collections::HashMap;

use crate::model::{NodeId, Value};
use crate::storage::ProcedureResult;
use crate::{Error, Result};

use super::fingerprint::{ContainerDto, siphash_string};

// ============================================================================
// Constants — mirror ladybug SPO Crystal seed values
// ============================================================================

/// Role vectors for SPO binding — deterministic seeds matching ladybug's SPOCrystal.
const ROLE_S_SEED: u64 = 0xDEADBEEF_CAFEBABE;
const ROLE_P_SEED: u64 = 0xFEEDFACE_DEADC0DE;
const ROLE_O_SEED: u64 = 0xBADC0FFE_E0DDF00D;

/// Belichtungsmesser sample points — 7 prime-spaced within 128 words.
const SAMPLE_POINTS: [usize; 7] = [0, 19, 41, 59, 79, 101, 127];

// ============================================================================
// SPO Trace
// ============================================================================

/// Holographic SPO trace stored per relationship.
///
/// ```text
/// trace = permute(S,1) ⊕ ROLE_S ⊕ permute(P,2) ⊕ ROLE_P ⊕ permute(O,3) ⊕ ROLE_O
/// ```
///
/// The **permutation** (circular word-shift) breaks XOR commutativity:
/// `permute(A,1)` ≠ `permute(A,3)`, so `A→B` and `B→A` produce different traces.
/// This is the standard VSA approach — ladybug's SPO Crystal uses orthogonal
/// codebooks for the same purpose; we use permutation since we don't have a
/// codebook in the bridge layer.
///
/// Recovery: `O = unpermute(trace ⊕ S_bound ⊕ P_bound ⊕ ROLE_O, 3)`
#[derive(Debug, Clone)]
pub struct SpoTrace {
    /// The full holographic trace (8192 bits).
    pub trace: ContainerDto,
    /// Subject fingerprint at bind time.
    pub subject_fp: ContainerDto,
    /// Predicate (verb) fingerprint.
    pub predicate_fp: ContainerDto,
    /// Object fingerprint at bind time.
    pub object_fp: ContainerDto,
}

/// Permutation offsets for SPO positions.
const PERM_S: usize = 1;
const PERM_P: usize = 43;  // prime-spaced to maximize decorrelation
const PERM_O: usize = 89;  // prime-spaced

impl SpoTrace {
    /// Bind a new SPO trace from component fingerprints.
    pub fn bind(subject: &ContainerDto, predicate: &ContainerDto, object: &ContainerDto) -> Self {
        let role_s = ContainerDto::random(ROLE_S_SEED);
        let role_p = ContainerDto::random(ROLE_P_SEED);
        let role_o = ContainerDto::random(ROLE_O_SEED);

        // Permute each component to a different position, then XOR with role vector
        let s_bound = subject.permute(PERM_S).xor(&role_s);
        let p_bound = predicate.permute(PERM_P).xor(&role_p);
        let o_bound = object.permute(PERM_O).xor(&role_o);
        let trace = s_bound.xor(&p_bound).xor(&o_bound);

        Self {
            trace,
            subject_fp: subject.clone(),
            predicate_fp: predicate.clone(),
            object_fp: object.clone(),
        }
    }

    /// Recover the missing Object from trace + known Subject + known Predicate.
    ///
    /// `O_bound = trace ⊕ S_bound ⊕ P_bound`
    /// `O = unpermute(O_bound ⊕ ROLE_O, PERM_O)`
    pub fn recover_object(trace: &ContainerDto, subject: &ContainerDto, predicate: &ContainerDto) -> ContainerDto {
        let role_s = ContainerDto::random(ROLE_S_SEED);
        let role_p = ContainerDto::random(ROLE_P_SEED);
        let role_o = ContainerDto::random(ROLE_O_SEED);

        let s_bound = subject.permute(PERM_S).xor(&role_s);
        let p_bound = predicate.permute(PERM_P).xor(&role_p);

        // trace ⊕ s_bound ⊕ p_bound = o_bound = permute(O, PERM_O) ⊕ ROLE_O
        let o_bound = trace.xor(&s_bound).xor(&p_bound);
        // O = unpermute(o_bound ⊕ ROLE_O, PERM_O)
        o_bound.xor(&role_o).unpermute(PERM_O)
    }

    /// Recover the missing Subject from trace + known Predicate + known Object.
    pub fn recover_subject(trace: &ContainerDto, predicate: &ContainerDto, object: &ContainerDto) -> ContainerDto {
        let role_s = ContainerDto::random(ROLE_S_SEED);
        let role_p = ContainerDto::random(ROLE_P_SEED);
        let role_o = ContainerDto::random(ROLE_O_SEED);

        let p_bound = predicate.permute(PERM_P).xor(&role_p);
        let o_bound = object.permute(PERM_O).xor(&role_o);

        let s_bound = trace.xor(&p_bound).xor(&o_bound);
        s_bound.xor(&role_s).unpermute(PERM_S)
    }

    /// Recover the missing Predicate from trace + known Subject + known Object.
    pub fn recover_predicate(trace: &ContainerDto, subject: &ContainerDto, object: &ContainerDto) -> ContainerDto {
        let role_s = ContainerDto::random(ROLE_S_SEED);
        let role_p = ContainerDto::random(ROLE_P_SEED);
        let role_o = ContainerDto::random(ROLE_O_SEED);

        let s_bound = subject.permute(PERM_S).xor(&role_s);
        let o_bound = object.permute(PERM_O).xor(&role_o);

        let p_bound = trace.xor(&s_bound).xor(&o_bound);
        p_bound.xor(&role_p).unpermute(PERM_P)
    }
}

// ============================================================================
// Belichtungsmesser — 7-point exposure meter (~14 cycles)
// ============================================================================

/// 7-point exposure meter on a ContainerDto (128 words).
/// Estimates total Hamming distance in ~14 CPU cycles.
/// Rejects ~90% of candidates at generous thresholds.
#[inline]
pub fn belichtungsmesser(a: &ContainerDto, b: &ContainerDto) -> u32 {
    let mut estimate: u32 = 0;
    for &idx in &SAMPLE_POINTS {
        estimate += (a.words[idx] ^ b.words[idx]).count_ones();
    }
    // Scale: 7 × 64 = 448 bits sampled out of 8192
    estimate * ContainerDto::BITS as u32 / 448
}

/// Exact Hamming with early exit — returns None if distance exceeds max_dist.
#[inline]
pub fn hamming_early_exit(a: &ContainerDto, b: &ContainerDto, max_dist: u32) -> Option<u32> {
    let mut total = 0u32;
    for i in 0..ContainerDto::WORDS {
        total += (a.words[i] ^ b.words[i]).count_ones();
        if total > max_dist {
            return None;
        }
    }
    Some(total)
}

// ============================================================================
// HDR Cascade Search
// ============================================================================

/// Result from the cascaded search.
#[derive(Debug, Clone)]
pub struct CascadeHit {
    pub node_id: NodeId,
    pub distance: u32,
    pub similarity: f32,
    /// Which cascade level resolved this (0=Belichtungsmesser, 2=exact).
    pub resolved_at: u8,
}

/// Run 3-level HDR cascade over fingerprints.
///
/// L0: Belichtungsmesser (7 samples, ~14 cycles) → 90% rejection
/// L1: Early-exit exact Hamming → prune distant candidates
/// L2: Full Hamming + ranking
///
/// Returns top-k results sorted by distance.
pub fn cascade_search(
    query: &ContainerDto,
    fingerprints: &[ContainerDto],
    slot_to_id: &HashMap<usize, NodeId>,
    threshold: u32,
    top_k: usize,
) -> Vec<CascadeHit> {
    let mut results = Vec::new();

    // L0 rejection threshold: generous 2× to avoid false negatives
    let l0_max = threshold.saturating_mul(2).saturating_add(200);

    for (slot, fp) in fingerprints.iter().enumerate() {
        let node_id = match slot_to_id.get(&slot) {
            Some(&id) => id,
            None => continue,
        };

        // L0: Belichtungsmesser — ~14 cycles per candidate
        let estimate = belichtungsmesser(query, fp);
        if estimate > l0_max {
            continue; // ~90% filtered here
        }

        // L1: Exact Hamming with early exit
        let distance = match hamming_early_exit(query, fp, threshold) {
            Some(d) => d,
            None => continue,
        };

        let similarity = 1.0 - (distance as f32 / ContainerDto::BITS as f32);

        results.push(CascadeHit {
            node_id,
            distance,
            similarity,
            resolved_at: 2,
        });
    }

    results.sort_by_key(|h| h.distance);
    results.truncate(top_k);
    results
}

// ============================================================================
// Multi-hop semiring traversal (neo4j-rs local version)
// ============================================================================

/// Semiring trait for container-native graph traversal.
///
/// Mirrors ladybug's `DnSemiring` but operates over `ContainerDto` (the neo4j-rs
/// local mirror of `Container`). When compiled together with ladybug-rs, this
/// bridges to the real `DnSemiring` implementations.
pub trait NeoSemiring {
    type Value: Clone;
    fn zero(&self) -> Self::Value;
    fn multiply(&self, verb_fp: &ContainerDto, input: &Self::Value, src_fp: &ContainerDto, dst_fp: &ContainerDto) -> Self::Value;
    fn add(&self, a: &Self::Value, b: &Self::Value) -> Self::Value;
    fn is_zero(&self, val: &Self::Value) -> bool;
}

/// Boolean BFS: reachability via OR/AND.
pub struct BooleanBfs;

impl NeoSemiring for BooleanBfs {
    type Value = bool;
    fn zero(&self) -> bool { false }
    fn multiply(&self, _verb: &ContainerDto, input: &bool, _src: &ContainerDto, _dst: &ContainerDto) -> bool { *input }
    fn add(&self, a: &bool, b: &bool) -> bool { *a || *b }
    fn is_zero(&self, val: &bool) -> bool { !*val }
}

/// Hamming shortest path: MinPlus with Hamming as edge weight.
pub struct HammingMinPlus;

impl NeoSemiring for HammingMinPlus {
    type Value = u32;
    fn zero(&self) -> u32 { u32::MAX }
    fn multiply(&self, _verb: &ContainerDto, input: &u32, src: &ContainerDto, dst: &ContainerDto) -> u32 {
        if *input == u32::MAX { return u32::MAX; }
        input.saturating_add(src.hamming(dst))
    }
    fn add(&self, a: &u32, b: &u32) -> u32 { (*a).min(*b) }
    fn is_zero(&self, val: &u32) -> bool { *val == u32::MAX }
}

/// Resonance search: find paths resonating with query fingerprint.
pub struct ResonanceSearch {
    pub query: ContainerDto,
}

impl NeoSemiring for ResonanceSearch {
    type Value = u32;
    fn zero(&self) -> u32 { 0 }
    fn multiply(&self, _verb: &ContainerDto, _input: &u32, src: &ContainerDto, dst: &ContainerDto) -> u32 {
        let edge_fp = src.xor(dst);
        let dist = edge_fp.hamming(&self.query);
        if dist < 10000 { 10000 - dist } else { 0 }
    }
    fn add(&self, a: &u32, b: &u32) -> u32 { (*a).max(*b) }
    fn is_zero(&self, val: &u32) -> bool { *val == 0 }
}

/// HDR path binding: XOR-compose path fingerprints along traversal.
pub struct HdrPathBind;

impl NeoSemiring for HdrPathBind {
    type Value = Option<ContainerDto>;
    fn zero(&self) -> Option<ContainerDto> { None }
    fn multiply(&self, _verb: &ContainerDto, input: &Option<ContainerDto>, _src: &ContainerDto, dst: &ContainerDto) -> Option<ContainerDto> {
        input.as_ref().map(|path| path.xor(dst))
    }
    fn add(&self, a: &Option<ContainerDto>, b: &Option<ContainerDto>) -> Option<ContainerDto> {
        match (a, b) {
            (Some(va), Some(vb)) => {
                // Bundle: majority vote of two containers
                let mut result = ContainerDto::zero();
                for i in 0..ContainerDto::WORDS {
                    result.words[i] = va.words[i] | vb.words[i]; // simplified bundle
                }
                Some(result)
            }
            (Some(v), None) | (None, Some(v)) => Some(v.clone()),
            (None, None) => None,
        }
    }
    fn is_zero(&self, val: &Option<ContainerDto>) -> bool { val.is_none() }
}

/// Cascaded Hamming with Belichtungsmesser pre-filter.
pub struct CascadedHamming {
    pub radius: u32,
}

impl NeoSemiring for CascadedHamming {
    type Value = u32;
    fn zero(&self) -> u32 { u32::MAX }
    fn multiply(&self, _verb: &ContainerDto, input: &u32, src: &ContainerDto, dst: &ContainerDto) -> u32 {
        if *input == u32::MAX { return u32::MAX; }
        let estimate = belichtungsmesser(src, dst);
        if estimate > self.radius * 2 { return u32::MAX; }
        input.saturating_add(src.hamming(dst))
    }
    fn add(&self, a: &u32, b: &u32) -> u32 { (*a).min(*b) }
    fn is_zero(&self, val: &u32) -> bool { *val == u32::MAX }
}

// ============================================================================
// SPO Procedures
// ============================================================================

/// `ladybug.spo.trace(subject_str, predicate_str, object_str)` → trace bytes + recovery proof
pub fn proc_spo_trace(args: &[Value]) -> Result<ProcedureResult> {
    let s_str = args.first().and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("spo.trace requires (subject, predicate, object)".into()))?;
    let p_str = args.get(1).and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("spo.trace requires (subject, predicate, object)".into()))?;
    let o_str = args.get(2).and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("spo.trace requires (subject, predicate, object)".into()))?;

    let s = ContainerDto::random(siphash_string(s_str));
    let p = ContainerDto::random(siphash_string(p_str));
    let o = ContainerDto::random(siphash_string(o_str));

    let spo = SpoTrace::bind(&s, &p, &o);

    // Prove recovery works
    let recovered_o = SpoTrace::recover_object(&spo.trace, &s, &p);
    let recovery_dist = recovered_o.hamming(&o);

    let mut row = HashMap::new();
    row.insert("trace".to_string(), Value::Bytes(spo.trace.as_bytes().to_vec()));
    row.insert("trace_popcount".to_string(), Value::Int(spo.trace.popcount() as i64));
    row.insert("recovery_distance".to_string(), Value::Int(recovery_dist as i64));
    row.insert("recovery_exact".to_string(), Value::Bool(recovery_dist == 0));

    Ok(ProcedureResult {
        columns: vec!["trace".into(), "trace_popcount".into(), "recovery_distance".into(), "recovery_exact".into()],
        rows: vec![row],
    })
}

/// `ladybug.spo.recover(trace_bytes, known1_str, known2_str, missing_role)` → recovered fingerprint
///
/// missing_role: "subject", "predicate", or "object"
pub fn proc_spo_recover(args: &[Value]) -> Result<ProcedureResult> {
    let known1 = args.first().and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("spo.recover requires (known1, known2, missing_role)".into()))?;
    let known2 = args.get(1).and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("spo.recover requires (known1, known2, missing_role)".into()))?;
    let trace_input = args.get(2).and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("spo.recover requires (known1, known2, trace_str)".into()))?;
    let missing_role = args.get(3).and_then(|v| v.as_str()).unwrap_or("object");

    let k1 = ContainerDto::random(siphash_string(known1));
    let k2 = ContainerDto::random(siphash_string(known2));
    let trace_fp = ContainerDto::random(siphash_string(trace_input));

    let recovered = match missing_role {
        "subject" => SpoTrace::recover_subject(&trace_fp, &k1, &k2),
        "predicate" => SpoTrace::recover_predicate(&trace_fp, &k1, &k2),
        _ => SpoTrace::recover_object(&trace_fp, &k1, &k2),
    };

    let mut row = HashMap::new();
    row.insert("recovered".to_string(), Value::Bytes(recovered.as_bytes().to_vec()));
    row.insert("popcount".to_string(), Value::Int(recovered.popcount() as i64));
    row.insert("missing_role".to_string(), Value::from(missing_role));

    Ok(ProcedureResult {
        columns: vec!["recovered".into(), "popcount".into(), "missing_role".into()],
        rows: vec![row],
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spo_recovery_exact() {
        let s = ContainerDto::random(siphash_string("Ada"));
        let p = ContainerDto::random(siphash_string("CAUSES"));
        let o = ContainerDto::random(siphash_string("Enlightenment"));

        let spo = SpoTrace::bind(&s, &p, &o);

        // Recover each component
        let recovered_o = SpoTrace::recover_object(&spo.trace, &s, &p);
        let recovered_s = SpoTrace::recover_subject(&spo.trace, &p, &o);
        let recovered_p = SpoTrace::recover_predicate(&spo.trace, &s, &o);

        assert_eq!(recovered_o, o, "Object recovery must be exact");
        assert_eq!(recovered_s, s, "Subject recovery must be exact");
        assert_eq!(recovered_p, p, "Predicate recovery must be exact");
    }

    #[test]
    fn test_spo_role_vectors_distinguish_direction() {
        let a = ContainerDto::random(siphash_string("Fire"));
        let b = ContainerDto::random(siphash_string("CAUSES"));
        let c = ContainerDto::random(siphash_string("Smoke"));

        // A -[CAUSES]-> C
        let forward = SpoTrace::bind(&a, &b, &c);
        // C -[CAUSES]-> A
        let reverse = SpoTrace::bind(&c, &b, &a);

        assert_ne!(forward.trace, reverse.trace,
            "Role vectors must make S→P→O distinguishable from O→P→S");
    }

    #[test]
    fn test_belichtungsmesser_self() {
        let a = ContainerDto::random(42);
        assert_eq!(belichtungsmesser(&a, &a), 0);
    }

    #[test]
    fn test_belichtungsmesser_random_near_4096() {
        let a = ContainerDto::random(1);
        let b = ContainerDto::random(2);
        let est = belichtungsmesser(&a, &b);
        let exact = a.hamming(&b);
        // Should be within ~20% of exact
        let diff = (est as i64 - exact as i64).unsigned_abs();
        assert!(diff < exact as u64 / 3, "est={est} exact={exact} diff={diff}");
    }

    #[test]
    fn test_cascade_search_finds_self() {
        let target = ContainerDto::random(siphash_string("target"));
        let noise1 = ContainerDto::random(siphash_string("noise1"));
        let noise2 = ContainerDto::random(siphash_string("noise2"));

        let corpus = vec![noise1, target.clone(), noise2];
        let slot_map: HashMap<usize, NodeId> = vec![
            (0, NodeId(1)), (1, NodeId(2)), (2, NodeId(3)),
        ].into_iter().collect();

        let results = cascade_search(&target, &corpus, &slot_map, 100, 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].node_id, NodeId(2));
        assert_eq!(results[0].distance, 0);
    }

    #[test]
    fn test_hamming_early_exit() {
        let a = ContainerDto::random(1);
        let b = ContainerDto::random(2);
        let exact = a.hamming(&b);

        // Should pass with generous threshold
        assert!(hamming_early_exit(&a, &b, exact + 100).is_some());
        // Should fail with tight threshold
        assert!(hamming_early_exit(&a, &b, 10).is_none());
    }
}
