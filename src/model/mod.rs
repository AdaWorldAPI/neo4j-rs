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
