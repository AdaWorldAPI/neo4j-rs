//! Relationship (edge) in the property graph.

use serde::{Deserialize, Serialize};
use super::{NodeId, PropertyMap, Value};

/// Opaque relationship identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelId(pub u64);

impl std::fmt::Display for RelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Traversal direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

/// A relationship (directed edge) in the property graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelId,
    /// Neo4j 5.x stable element identifier (e.g. `"5:abc:456"`).
    pub element_id: Option<String>,
    pub src: NodeId,
    pub dst: NodeId,
    pub rel_type: String,
    pub properties: PropertyMap,
}

impl Relationship {
    pub fn new(id: RelId, src: NodeId, dst: NodeId, rel_type: impl Into<String>) -> Self {
        Self {
            id,
            element_id: None,
            src,
            dst,
            rel_type: rel_type.into(),
            properties: PropertyMap::new(),
        }
    }

    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    /// The "other" end of the relationship from the given node.
    pub fn other_node(&self, from: NodeId) -> Option<NodeId> {
        if from == self.src { Some(self.dst) }
        else if from == self.dst { Some(self.src) }
        else { None }
    }
}
