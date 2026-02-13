//! Node in the property graph.

use serde::{Deserialize, Serialize};
use super::{PropertyMap, Value};

/// Opaque node identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A node in the property graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// Neo4j 5.x stable element identifier (e.g. `"4:abc:123"`).
    pub element_id: Option<String>,
    pub labels: Vec<String>,
    pub properties: PropertyMap,
}

impl Node {
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            element_id: None,
            labels: Vec::new(),
            properties: PropertyMap::new(),
        }
    }

    pub fn with_labels(mut self, labels: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    pub fn has_label(&self, label: &str) -> bool {
        self.labels.iter().any(|l| l == label)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }
}
