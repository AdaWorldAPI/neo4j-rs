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

    /// Returns an iterator over (node, relationship, node) triples along the path.
    pub fn triples(&self) -> impl Iterator<Item = (&Node, &Relationship, &Node)> {
        self.relationships.iter().enumerate().map(move |(i, rel)| {
            (&self.nodes[i], rel, &self.nodes[i + 1])
        })
    }

    /// Reverse the path direction.
    pub fn reverse(&mut self) {
        self.nodes.reverse();
        self.relationships.reverse();
    }

    /// Check if a node (by id) exists anywhere in the path.
    pub fn contains_node(&self, id: crate::model::NodeId) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    /// Check if a relationship (by id) exists in the path.
    pub fn contains_relationship(&self, id: crate::model::RelId) -> bool {
        self.relationships.iter().any(|r| r.id == id)
    }

    /// Get a node at a specific position in the path (0-indexed).
    pub fn node_at(&self, index: usize) -> Option<&Node> {
        self.nodes.get(index)
    }

    /// Get a relationship at a specific position in the path (0-indexed).
    pub fn relationship_at(&self, index: usize) -> Option<&Relationship> {
        self.relationships.get(index)
    }

    /// Return all distinct node IDs in the path.
    pub fn node_ids(&self) -> Vec<crate::model::NodeId> {
        self.nodes.iter().map(|n| n.id).collect()
    }

    /// Return all relationship IDs in the path.
    pub fn relationship_ids(&self) -> Vec<crate::model::RelId> {
        self.relationships.iter().map(|r| r.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn test_node(id: u64) -> Node {
        Node::new(NodeId(id))
    }

    fn test_rel(id: u64, src: u64, dst: u64) -> Relationship {
        Relationship::new(RelId(id), NodeId(src), NodeId(dst), "KNOWS")
    }

    #[test]
    fn test_path_triples() {
        let mut path = Path::single(test_node(1));
        path.append(test_rel(10, 1, 2), test_node(2));
        path.append(test_rel(11, 2, 3), test_node(3));

        let triples: Vec<_> = path.triples().collect();
        assert_eq!(triples.len(), 2);
        assert_eq!(triples[0].0.id, NodeId(1));
        assert_eq!(triples[0].2.id, NodeId(2));
        assert_eq!(triples[1].0.id, NodeId(2));
        assert_eq!(triples[1].2.id, NodeId(3));
    }

    #[test]
    fn test_path_contains() {
        let mut path = Path::single(test_node(1));
        path.append(test_rel(10, 1, 2), test_node(2));

        assert!(path.contains_node(NodeId(1)));
        assert!(path.contains_node(NodeId(2)));
        assert!(!path.contains_node(NodeId(99)));
        assert!(path.contains_relationship(RelId(10)));
        assert!(!path.contains_relationship(RelId(99)));
    }

    #[test]
    fn test_path_reverse() {
        let mut path = Path::single(test_node(1));
        path.append(test_rel(10, 1, 2), test_node(2));
        path.append(test_rel(11, 2, 3), test_node(3));

        assert_eq!(path.start().id, NodeId(1));
        assert_eq!(path.end().id, NodeId(3));

        path.reverse();
        assert_eq!(path.start().id, NodeId(3));
        assert_eq!(path.end().id, NodeId(1));
    }

    #[test]
    fn test_path_node_at() {
        let mut path = Path::single(test_node(1));
        path.append(test_rel(10, 1, 2), test_node(2));

        assert_eq!(path.node_at(0).unwrap().id, NodeId(1));
        assert_eq!(path.node_at(1).unwrap().id, NodeId(2));
        assert!(path.node_at(5).is_none());
    }
}
