//! Path — a sequence of alternating nodes and relationships.

use serde::{Deserialize, Serialize};
use super::{Node, Relationship};

/// A path in the graph: node -[rel]-> node -[rel]-> node ...
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Path {
    /// Nodes along the path. Always has one more element than `relationships`.
    pub nodes: Vec<Node>,
    /// Relationships connecting consecutive nodes.
    pub relationships: Vec<Relationship>,
}

impl Path {
    pub fn single(node: Node) -> Self {
        Self { nodes: vec![node], relationships: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.relationships.len()
    }

    pub fn is_empty(&self) -> bool {
        self.relationships.is_empty()
    }

    pub fn start(&self) -> &Node {
        self.nodes.first().expect("Path always has at least one node")
    }

    pub fn end(&self) -> &Node {
        self.nodes.last().expect("Path always has at least one node")
    }

    /// Extend path with a relationship and its target node.
    pub fn append(&mut self, rel: Relationship, node: Node) {
        self.relationships.push(rel);
        self.nodes.push(node);
    }
}
