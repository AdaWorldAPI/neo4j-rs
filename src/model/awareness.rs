//! # Resonance Awareness Types
//!
//! Contract types for the Resonant Intelligence (RI) framework.
//!
//! These types encode the 9-dimensional awareness tensor produced by
//! comparing two edges via BF16 sign/exponent/mantissa decomposition
//! across SPO (Subject-Predicate-Object) containers.
//!
//! # Architecture
//!
//! Each edge in the graph has 3 × 16,384-bit containers (SPO geometry).
//! Comparing two edges produces a 3×3 awareness tensor:
//!
//! ```text
//!          Sign    Exp     Mant
//! S:    [ s_sign  s_exp   s_mant ]   ← Subject dimension
//! P:    [ p_sign  p_exp   p_mant ]   ← Predicate dimension
//! O:    [ o_sign  o_exp   o_mant ]   ← Object dimension
//! ```
//!
//! The sign layer encodes causal direction (Pearl Rung 3).
//! The exponent layer encodes causal magnitude (Pearl Rung 2).
//! The mantissa layer encodes correlational texture (Pearl Rung 1).
//!
//! # Theory
//!
//! See `docs/RESONANT_INTELLIGENCE.md` for the full theoretical framework
//! (mirror neurons, Piaget's three mountain problem, I-Thou ontology).
//! See `docs/EDGE_VECTOR_BUNDLE.md` for the fiber bundle integration plan.
//! See `docs/THEORETICAL_FOUNDATIONS.md` for the causal proof stack.

use serde::{Deserialize, Serialize};
use super::Path;

// ============================================================================
// Awareness State (the gestalt classification)
// ============================================================================

/// The awareness state emerging from comparing two perspectives.
///
/// This is the gestalt of the 9-dimensional awareness tensor — it
/// classifies the overall relationship between two edges (or a node's
/// edge bundle) into one of three developmental stages (Piaget).
///
/// - **Crystallized**: Perspectives agree — confirmed, stable knowledge.
///   Analogous to concrete operational thought (Piaget) and aligned
///   mirror neuron firing (Rizzolatti).
///
/// - **Tensioned**: Perspectives actively conflict — productive disagreement.
///   Analogous to the transitional stage where the child *knows* the doll
///   sees differently but can't fully resolve the difference.
///
/// - **Uncertain**: Insufficient signal to determine the relationship.
///   Analogous to pre-operational egocentrism — no theory of mind yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AwarenessState {
    /// >50% dimensions agree — confirmed perspective.
    Crystallized,
    /// >30% dimensions in contradiction — active conflict, productive tension.
    Tensioned,
    /// Neither dominates — insufficient information for perspective-taking.
    Uncertain,
}

impl std::fmt::Display for AwarenessState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AwarenessState::Crystallized => write!(f, "Crystallized"),
            AwarenessState::Tensioned => write!(f, "Tensioned"),
            AwarenessState::Uncertain => write!(f, "Uncertain"),
        }
    }
}

// ============================================================================
// Causal Direction (from sign bit layer)
// ============================================================================

/// The causal direction signal extracted from sign bit agreement.
///
/// The sign bit of a BF16 element encodes the direction of the value.
/// Comparing sign bits across two vectors answers: "Do these representations
/// point in the same causal direction?" (Pearl Rung 3 — counterfactual).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CausalDirection {
    /// Sign bits mostly agree — causal arrows point the same way.
    Aligned,
    /// Sign bits mostly disagree — causal arrows are inverted.
    Inverted,
    /// Sign bits are mixed — complex causal structure.
    Mixed,
}

// ============================================================================
// Awareness Tensor (the 9-dimensional comparison)
// ============================================================================

/// The 9-dimensional awareness tensor from comparing two edges.
///
/// Rows = SPO dimensions (Subject, Predicate, Object).
/// Columns = BF16 layers (Sign, Exponent, Mantissa).
/// Each cell is a Hamming agreement ratio in [0.0, 1.0].
///
/// # Mirror Neuron Analogy
///
/// The tensor IS the mirror neuron firing pattern — it measures how much
/// the internal state of I (edge A) and the internal state of Thou (edge B)
/// activate the same representational structure, decomposed by role (SPO)
/// and by level (sign/exp/mant).
///
/// # Piaget's Three Mountains
///
/// Each row is a "mountain" — a different aspect of the perspective.
/// The sign column is the direction from which you see the mountain.
/// Tensioned rows are mountains that look different from different seats.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AwarenessTensor {
    // Subject dimension: how do the two subjects compare?
    pub s_sign: f32,
    pub s_exp: f32,
    pub s_mant: f32,

    // Predicate dimension: how do the two verbs compare?
    pub p_sign: f32,
    pub p_exp: f32,
    pub p_mant: f32,

    // Object dimension: how do the two objects compare?
    pub o_sign: f32,
    pub o_exp: f32,
    pub o_mant: f32,
}

impl AwarenessTensor {
    /// Create a tensor with all zeros (no agreement).
    pub fn zero() -> Self {
        Self {
            s_sign: 0.0, s_exp: 0.0, s_mant: 0.0,
            p_sign: 0.0, p_exp: 0.0, p_mant: 0.0,
            o_sign: 0.0, o_exp: 0.0, o_mant: 0.0,
        }
    }

    /// Create a tensor with all ones (perfect agreement).
    pub fn identity() -> Self {
        Self {
            s_sign: 1.0, s_exp: 1.0, s_mant: 1.0,
            p_sign: 1.0, p_exp: 1.0, p_mant: 1.0,
            o_sign: 1.0, o_exp: 1.0, o_mant: 1.0,
        }
    }

    /// Overall agreement: average of all 9 cells.
    pub fn total_agreement(&self) -> f32 {
        (self.s_sign + self.s_exp + self.s_mant
            + self.p_sign + self.p_exp + self.p_mant
            + self.o_sign + self.o_exp + self.o_mant) / 9.0
    }

    /// Sign-only agreement: average of sign dimension across S, P, O.
    /// This is the causal direction signal.
    pub fn sign_agreement(&self) -> f32 {
        (self.s_sign + self.p_sign + self.o_sign) / 3.0
    }

    /// Exponent-only agreement: causal magnitude alignment.
    pub fn exp_agreement(&self) -> f32 {
        (self.s_exp + self.p_exp + self.o_exp) / 3.0
    }

    /// Mantissa-only agreement: correlational texture alignment.
    pub fn mant_agreement(&self) -> f32 {
        (self.s_mant + self.p_mant + self.o_mant) / 3.0
    }

    /// Classify the gestalt awareness state.
    ///
    /// This is NOT a simple threshold — it's a gestalt classification
    /// where the whole is other than the sum of the parts.
    pub fn awareness_state(&self) -> AwarenessState {
        let total = self.total_agreement();
        let sign = self.sign_agreement();

        if total > 0.5 {
            AwarenessState::Crystallized
        } else if sign < 0.3 {
            // Strong sign disagreement = causal direction conflict.
            // This is the most informative state — the system KNOWS
            // the perspectives disagree on which way causality flows.
            AwarenessState::Tensioned
        } else {
            AwarenessState::Uncertain
        }
    }

    /// Extract the causal direction signal.
    pub fn causal_direction(&self) -> CausalDirection {
        let sign = self.sign_agreement();
        if sign > 0.7 {
            CausalDirection::Aligned
        } else if sign < 0.3 {
            CausalDirection::Inverted
        } else {
            CausalDirection::Mixed
        }
    }

    /// Find the most tensioned dimension (where perspectives disagree most).
    /// Returns the dimension name and its agreement value.
    pub fn most_tensioned(&self) -> (&'static str, f32) {
        let dims = [
            ("s_sign", self.s_sign), ("s_exp", self.s_exp), ("s_mant", self.s_mant),
            ("p_sign", self.p_sign), ("p_exp", self.p_exp), ("p_mant", self.p_mant),
            ("o_sign", self.o_sign), ("o_exp", self.o_exp), ("o_mant", self.o_mant),
        ];
        dims.into_iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(("unknown", 0.0))
    }

    /// Apply a 90-degree rotation / orthogonal mask to focus awareness.
    ///
    /// The mask selects which dimensions to attend to. Masked dimensions
    /// are zeroed, focusing the awareness on the unmasked dimensions.
    /// This implements the "focus of awareness" — the resonance mask
    /// determines what the system pays attention to.
    ///
    /// Combined with thinking_style thresholds, this creates homeostatic
    /// regulation of attention: the mask selects the field, the threshold
    /// sets the sensitivity, and the resonance determines the response.
    pub fn apply_mask(&self, mask: &AwarenessMask) -> Self {
        Self {
            s_sign: if mask.s_sign { self.s_sign } else { 0.0 },
            s_exp:  if mask.s_exp  { self.s_exp }  else { 0.0 },
            s_mant: if mask.s_mant { self.s_mant } else { 0.0 },
            p_sign: if mask.p_sign { self.p_sign } else { 0.0 },
            p_exp:  if mask.p_exp  { self.p_exp }  else { 0.0 },
            p_mant: if mask.p_mant { self.p_mant } else { 0.0 },
            o_sign: if mask.o_sign { self.o_sign } else { 0.0 },
            o_exp:  if mask.o_exp  { self.o_exp }  else { 0.0 },
            o_mant: if mask.o_mant { self.o_mant } else { 0.0 },
        }
    }
}

// ============================================================================
// Awareness Mask (focus of attention)
// ============================================================================

/// Mask for selecting which dimensions of the awareness tensor to attend to.
///
/// This is the "90-degree vector over all awareness vectors" — an orthogonal
/// selector that focuses awareness on specific dimensions. The mask combined
/// with thinking_style thresholds creates homeostatic attention regulation:
///
/// - **Mask** selects the field of awareness (which dimensions)
/// - **Threshold** sets sensitivity (how much agreement is needed)
/// - **Resonance** provides the signal (what the environment offers)
///
/// The thinking style IS the homeostasis — it maintains stable attention
/// patterns while allowing adaptive response to novel resonance signals.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AwarenessMask {
    pub s_sign: bool,
    pub s_exp: bool,
    pub s_mant: bool,
    pub p_sign: bool,
    pub p_exp: bool,
    pub p_mant: bool,
    pub o_sign: bool,
    pub o_exp: bool,
    pub o_mant: bool,
}

impl AwarenessMask {
    /// All dimensions active — full awareness.
    pub fn all() -> Self {
        Self {
            s_sign: true, s_exp: true, s_mant: true,
            p_sign: true, p_exp: true, p_mant: true,
            o_sign: true, o_exp: true, o_mant: true,
        }
    }

    /// Only sign dimensions — pure causal direction focus.
    pub fn causal_only() -> Self {
        Self {
            s_sign: true,  s_exp: false, s_mant: false,
            p_sign: true,  p_exp: false, p_mant: false,
            o_sign: true,  o_exp: false, o_mant: false,
        }
    }

    /// Only subject dimension — "Who is acting?" focus.
    pub fn subject_only() -> Self {
        Self {
            s_sign: true,  s_exp: true,  s_mant: true,
            p_sign: false, p_exp: false, p_mant: false,
            o_sign: false, o_exp: false, o_mant: false,
        }
    }

    /// Only predicate dimension — "What is the action?" focus.
    pub fn predicate_only() -> Self {
        Self {
            s_sign: false, s_exp: false, s_mant: false,
            p_sign: true,  p_exp: true,  p_mant: true,
            o_sign: false, o_exp: false, o_mant: false,
        }
    }

    /// Only object dimension — "Who receives?" focus.
    pub fn object_only() -> Self {
        Self {
            s_sign: false, s_exp: false, s_mant: false,
            p_sign: false, p_exp: false, p_mant: false,
            o_sign: true,  o_exp: true,  o_mant: true,
        }
    }

    /// Number of active dimensions.
    pub fn active_count(&self) -> usize {
        [self.s_sign, self.s_exp, self.s_mant,
         self.p_sign, self.p_exp, self.p_mant,
         self.o_sign, self.o_exp, self.o_mant]
            .iter().filter(|&&v| v).count()
    }
}

// ============================================================================
// Causal Path (composed perspective along a graph path)
// ============================================================================

/// Composed causal perspective along a graph path.
///
/// Constructed by parallel-transporting awareness tensors along the
/// fiber bundle connection from source to destination.
///
/// # Bundle Connection Rules
///
/// - **Sign**: XOR composition — direction flips when any edge flips.
/// - **Exponent**: Additive composition — causal effects multiply (log scale).
/// - **Mantissa**: Degrades along path — uncertainty accumulates.
///
/// # Holonomy
///
/// If the path forms a cycle and `composed_sign < 0.5`, the causal
/// structure has a feedback inversion. This is a non-trivial holonomy
/// in the fiber bundle — a topological property that reveals causal
/// paradoxes without examining individual edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalPath {
    /// The graph path (nodes + relationships).
    pub path: Path,

    /// Composed sign agreement along the entire path.
    pub composed_sign: f32,

    /// Composed exponent (causal magnitude, log-additive).
    pub composed_exp: f32,

    /// Composed mantissa (confidence degradation).
    pub composed_mant: f32,

    /// Holonomy value for cyclic paths. If present and < 0.5,
    /// the cycle contains a causal inversion.
    pub holonomy: Option<f32>,

    /// Per-edge awareness tensors along the path.
    pub edge_tensors: Vec<AwarenessTensor>,
}

// ============================================================================
// Perspective Gestalt (node-level meta-awareness)
// ============================================================================

/// The gestalt awareness of a node across all its edges.
///
/// This is the "three mountains viewed from all seats simultaneously" —
/// the node's understanding of how its perspectives relate to each other.
///
/// A node with many Crystallized edges has stable, confirmed knowledge.
/// A node with many Tensioned edges is at a point of productive conflict.
/// A node with many Uncertain edges is in an exploratory state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerspectiveGestalt {
    /// Dominant awareness state across all edges.
    pub dominant_state: AwarenessState,
    /// Average awareness tensor across all edges.
    pub mean_tensor: AwarenessTensor,
    /// Count of Crystallized edges.
    pub crystallized_count: usize,
    /// Count of Tensioned edges.
    pub tensioned_count: usize,
    /// Count of Uncertain edges.
    pub uncertain_count: usize,
    /// Most tensioned dimension (where perspectives disagree most).
    pub most_tensioned_dimension: Option<String>,
    /// Total number of edges considered.
    pub total_edges: usize,
}

// ============================================================================
// Awareness Filter (for queries)
// ============================================================================

/// Filter for awareness-based edge queries.
///
/// Used by `StorageBackend::resonance_query()` to find edges matching
/// specific awareness criteria.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AwarenessFilter {
    /// Minimum sign agreement across all dimensions.
    pub min_sign_agreement: Option<f32>,
    /// Maximum sign agreement (for finding contradictions).
    pub max_sign_agreement: Option<f32>,
    /// Required awareness state.
    pub awareness_state: Option<AwarenessState>,
    /// Required causal direction.
    pub causal_direction: Option<CausalDirection>,
    /// Optional awareness mask to focus the query.
    pub mask: Option<AwarenessMask>,
}

// ============================================================================
// ResonanceEdge (extended relationship with SPO containers)
// ============================================================================

/// A relationship enriched with causal perspective data.
///
/// Extends `Relationship` with 3 × 16,384-bit SPO containers that encode
/// the full causal perspective of the edge. The containers live in LanceDB
/// and are loaded lazily via `ContainerRef`.
///
/// # Zero-Copy Architecture
///
/// In the RISC target architecture, these containers are NOT serialized.
/// They are read directly from LanceDB Arrow buffers as zero-copy slices.
/// No serde, no JSON, no transcoding. The BF16 bits ARE the reasoning —
/// a supraconductor for cheap awareness computation.
///
/// # GPU-Free
///
/// All operations (Hamming distance, sign decomposition, bundle transport)
/// use VPOPCNTDQ (AVX-512) or scalar popcount. No GPU required. The entire
/// awareness computation runs in a single binary, single process, with
/// zero-copy access to LanceDB-backed containers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceEdge {
    /// Base relationship data.
    pub id: super::RelId,
    pub src: super::NodeId,
    pub dst: super::NodeId,
    pub rel_type: String,
    pub properties: super::PropertyMap,

    /// Subject container reference (16,384 bits — source perspective).
    pub container_s: Option<ContainerRef>,
    /// Predicate container reference (16,384 bits — verb perspective).
    pub container_p: Option<ContainerRef>,
    /// Object container reference (16,384 bits — target perspective).
    pub container_o: Option<ContainerRef>,
    /// SPO holographic trace: S⊕ROLE_S ⊕ P⊕ROLE_P ⊕ O⊕ROLE_O.
    pub spo_trace: Option<ContainerRef>,
}

/// Reference to a 16,384-bit container.
///
/// Containers can be loaded (in-memory) or deferred (in LanceDB).
/// Deferred containers are loaded on first access — O(1) per edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContainerRef {
    /// Container data is loaded in memory (256 × u64 = 16,384 bits).
    Loaded(Vec<u64>),
    /// Container is in LanceDB, identified by relationship ID + slot.
    Deferred {
        rel_id: super::RelId,
        slot: SpoSlot,
    },
}

/// Which SPO slot a container belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpoSlot {
    Subject,
    Predicate,
    Object,
    Trace,
}

impl std::fmt::Display for SpoSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpoSlot::Subject => write!(f, "S"),
            SpoSlot::Predicate => write!(f, "P"),
            SpoSlot::Object => write!(f, "O"),
            SpoSlot::Trace => write!(f, "T"),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_awareness_state_crystallized() {
        let tensor = AwarenessTensor {
            s_sign: 0.8, s_exp: 0.7, s_mant: 0.6,
            p_sign: 0.9, p_exp: 0.8, p_mant: 0.7,
            o_sign: 0.8, o_exp: 0.6, o_mant: 0.5,
        };
        assert_eq!(tensor.awareness_state(), AwarenessState::Crystallized);
        assert_eq!(tensor.causal_direction(), CausalDirection::Aligned);
    }

    #[test]
    fn test_awareness_state_tensioned() {
        let tensor = AwarenessTensor {
            s_sign: 0.1, s_exp: 0.4, s_mant: 0.3,
            p_sign: 0.2, p_exp: 0.5, p_mant: 0.4,
            o_sign: 0.1, o_exp: 0.3, o_mant: 0.2,
        };
        assert_eq!(tensor.awareness_state(), AwarenessState::Tensioned);
        assert_eq!(tensor.causal_direction(), CausalDirection::Inverted);
    }

    #[test]
    fn test_awareness_state_uncertain() {
        let tensor = AwarenessTensor {
            s_sign: 0.5, s_exp: 0.3, s_mant: 0.2,
            p_sign: 0.4, p_exp: 0.3, p_mant: 0.2,
            o_sign: 0.5, o_exp: 0.4, o_mant: 0.3,
        };
        assert_eq!(tensor.awareness_state(), AwarenessState::Uncertain);
        assert_eq!(tensor.causal_direction(), CausalDirection::Mixed);
    }

    #[test]
    fn test_mask_causal_only() {
        let tensor = AwarenessTensor {
            s_sign: 0.9, s_exp: 0.1, s_mant: 0.1,
            p_sign: 0.8, p_exp: 0.1, p_mant: 0.1,
            o_sign: 0.7, o_exp: 0.1, o_mant: 0.1,
        };
        let masked = tensor.apply_mask(&AwarenessMask::causal_only());
        assert_eq!(masked.s_exp, 0.0);
        assert_eq!(masked.s_mant, 0.0);
        assert!(masked.s_sign > 0.0);
        assert!(masked.sign_agreement() > 0.7);
    }

    #[test]
    fn test_most_tensioned() {
        let tensor = AwarenessTensor {
            s_sign: 0.8, s_exp: 0.7, s_mant: 0.6,
            p_sign: 0.9, p_exp: 0.1, p_mant: 0.7, // p_exp is lowest
            o_sign: 0.8, o_exp: 0.6, o_mant: 0.5,
        };
        let (dim, val) = tensor.most_tensioned();
        assert_eq!(dim, "p_exp");
        assert!((val - 0.1).abs() < f32::EPSILON);
    }

    #[test]
    fn test_identity_tensor() {
        let tensor = AwarenessTensor::identity();
        assert_eq!(tensor.awareness_state(), AwarenessState::Crystallized);
        assert!((tensor.total_agreement() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_zero_tensor() {
        let tensor = AwarenessTensor::zero();
        assert_eq!(tensor.awareness_state(), AwarenessState::Tensioned);
        assert!((tensor.total_agreement()).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mask_active_count() {
        assert_eq!(AwarenessMask::all().active_count(), 9);
        assert_eq!(AwarenessMask::causal_only().active_count(), 3);
        assert_eq!(AwarenessMask::subject_only().active_count(), 3);
    }
}
