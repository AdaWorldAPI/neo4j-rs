//! # Property Graph Model
//!
//! Clean DTOs that define the Neo4j-compatible property graph.
//! These types cross every boundary: storage ↔ planner ↔ execution ↔ user.
//!
//! Design rule: NO holograph types, NO Lance types, NO Arrow types here.
//! This module is pure data — no I/O, no state, no async.

pub mod node;
pub mod relationship;
pub mod path;
pub mod value;
pub mod property_map;
pub mod awareness;
pub mod bf16_distance;

pub use node::{Node, NodeId};
pub use relationship::{Relationship, RelId, Direction};
pub use path::Path;
pub use value::Value;
pub use property_map::PropertyMap;
pub use awareness::{
    AwarenessState, AwarenessTensor, AwarenessMask, AwarenessFilter,
    CausalDirection, CausalPath, PerspectiveGestalt,
    ResonanceEdge, ContainerRef, SpoSlot,
};
pub use bf16_distance::{
    Bf16Distance, LayerCounts, SpoDistance,
    structured_bf16_distance, structured_bf16_distance_u16, spo_distance,
    qualia_to_bf16, bf16_to_qualia, qualia_vec_to_bf16, bf16_vec_to_qualia,
    W_SIGN, W_EXP, W_MANT, EXP_GATE, ELEMENTS_PER_CONTAINER, BIAS_OFFSET,
    // Nib4: 4-bit per-dimension qualia encoding
    Nib4Codebook, SpoNib4Distance,
    nib4_distance, nib4_distance_packed, nib4_distance_normalized,
    nib4_distance_bf16_aligned, nib4_full_distance, nib4_intensity_differs,
    nib4_pack_bf16, nib4_unpack_bf16,
    nib4_to_hex, spo_nib4_distance,
    NIB4_LEVELS, QUALIA_DIMS, QUALIA_DIM_NAMES, QUALIA_JSON_KEYS,
    QUALIA_BITS, QUALIA_WORDS, INTENSITY_WORD, INTENSITY_BIT, TOPOLOGY_BITS,
};
