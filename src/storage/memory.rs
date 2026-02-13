//! In-memory storage backend.
//!
//! This is the reference implementation of `StorageBackend`.
//! It uses simple HashMaps protected by RwLock.
//!
//! ## Limitations
//!
//! - **No real transactions**: `commit_tx()` and `rollback_tx()` are no-ops.
//!   Writes are applied immediately. Rollback does NOT undo mutations.
//! - **Single-writer only**: Per-collection locks mean multi-step mutations
//!   are NOT atomic. Safe for single-threaded or read-heavy use only.
//! - **No property indexes**: `create_index()` is a no-op. All property
//!   lookups do a full scan.
//!
//! Use this backend for:
//! - Testing the Cypher parser, planner, and execution engine
//! - Embedding neo4j-rs in applications that don't need persistence
//! - Validating correctness before running against ladybug-rs or Neo4j

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use async_trait::async_trait;

use crate::model::*;
use crate::tx::{Transaction, TxMode, TxId};
use crate::index::IndexType;
use crate::{Error, Result};
use super::{StorageBackend, ExpandDepth};

// ============================================================================
// MemoryBackend
// ============================================================================

/// In-memory property graph storage.
pub struct MemoryBackend {
    inner: Arc<MemoryInner>,
}

struct MemoryInner {
    nodes: RwLock<HashMap<NodeId, Node>>,
    relationships: RwLock<HashMap<RelId, Relationship>>,
    /// node_id → list of relationship IDs
    adjacency: RwLock<HashMap<NodeId, Vec<RelId>>>,
    /// label → set of node IDs (poor man's label index)
    label_index: RwLock<HashMap<String, Vec<NodeId>>>,
    next_node_id: AtomicU64,
    next_rel_id: AtomicU64,
    next_tx_id: AtomicU64,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MemoryInner {
                nodes: RwLock::new(HashMap::new()),
                relationships: RwLock::new(HashMap::new()),
                adjacency: RwLock::new(HashMap::new()),
                label_index: RwLock::new(HashMap::new()),
                next_node_id: AtomicU64::new(1),
                next_rel_id: AtomicU64::new(1),
                next_tx_id: AtomicU64::new(1),
            }),
        }
    }
}

// ============================================================================
// MemoryTx
// ============================================================================

/// In-memory transaction (currently just a marker — no real MVCC).
pub struct MemoryTx {
    id: TxId,
    mode: TxMode,
}

impl Transaction for MemoryTx {
    fn mode(&self) -> TxMode { self.mode }
    fn id(&self) -> TxId { self.id }
}

// ============================================================================
// StorageBackend impl
// ============================================================================

#[async_trait]
impl StorageBackend for MemoryBackend {
    type Tx = MemoryTx;

    async fn shutdown(&self) -> Result<()> { Ok(()) }

    async fn begin_tx(&self, mode: TxMode) -> Result<MemoryTx> {
        let id = TxId(self.inner.next_tx_id.fetch_add(1, Ordering::Relaxed));
        Ok(MemoryTx { id, mode })
    }

    /// No-op: memory backend applies writes immediately, not on commit.
    async fn commit_tx(&self, _tx: MemoryTx) -> Result<()> { Ok(()) }

    /// WARNING: No-op. Memory backend has no write-ahead log.
    /// Mutations applied during this transaction are NOT reverted.
    async fn rollback_tx(&self, _tx: MemoryTx) -> Result<()> { Ok(()) }

    // ========================================================================
    // Node CRUD
    // ========================================================================

    async fn create_node(
        &self,
        _tx: &mut MemoryTx,
        labels: &[&str],
        props: PropertyMap,
    ) -> Result<NodeId> {
        let id = NodeId(self.inner.next_node_id.fetch_add(1, Ordering::Relaxed));
        let node = Node {
            id,
            element_id: None,
            labels: labels.iter().map(|l| l.to_string()).collect(),
            properties: props,
        };

        // Update label index
        {
            let mut idx = self.inner.label_index.write();
            for label in &node.labels {
                idx.entry(label.clone()).or_default().push(id);
            }
        }

        self.inner.nodes.write().insert(id, node);
        self.inner.adjacency.write().insert(id, Vec::new());

        Ok(id)
    }

    async fn get_node(&self, _tx: &MemoryTx, id: NodeId) -> Result<Option<Node>> {
        Ok(self.inner.nodes.read().get(&id).cloned())
    }

    async fn delete_node(&self, _tx: &mut MemoryTx, id: NodeId) -> Result<bool> {
        // Check for existing relationships (Neo4j semantics: can't delete connected node)
        {
            let adj = self.inner.adjacency.read();
            if let Some(rels) = adj.get(&id) {
                if !rels.is_empty() {
                    return Err(Error::ConstraintViolation(
                        format!("Cannot delete node {id} with {} relationships. Delete relationships first.", rels.len())
                    ));
                }
            }
        }

        let removed = self.inner.nodes.write().remove(&id);
        self.inner.adjacency.write().remove(&id);

        if let Some(node) = &removed {
            let mut idx = self.inner.label_index.write();
            for label in &node.labels {
                if let Some(ids) = idx.get_mut(label) {
                    ids.retain(|nid| *nid != id);
                }
            }
        }

        Ok(removed.is_some())
    }

    async fn set_node_property(
        &self,
        _tx: &mut MemoryTx,
        id: NodeId,
        key: &str,
        val: Value,
    ) -> Result<()> {
        let mut nodes = self.inner.nodes.write();
        let node = nodes.get_mut(&id).ok_or_else(|| Error::NotFound(format!("Node {id}")))?;
        node.properties.insert(key.to_string(), val);
        Ok(())
    }

    async fn remove_node_property(
        &self,
        _tx: &mut MemoryTx,
        id: NodeId,
        key: &str,
    ) -> Result<()> {
        let mut nodes = self.inner.nodes.write();
        let node = nodes.get_mut(&id).ok_or_else(|| Error::NotFound(format!("Node {id}")))?;
        node.properties.remove(key);
        Ok(())
    }

    async fn add_label(&self, _tx: &mut MemoryTx, id: NodeId, label: &str) -> Result<()> {
        let mut nodes = self.inner.nodes.write();
        let node = nodes.get_mut(&id).ok_or_else(|| Error::NotFound(format!("Node {id}")))?;
        if !node.labels.contains(&label.to_string()) {
            node.labels.push(label.to_string());
            drop(nodes);
            self.inner.label_index.write().entry(label.to_string()).or_default().push(id);
        }
        Ok(())
    }

    async fn remove_label(&self, _tx: &mut MemoryTx, id: NodeId, label: &str) -> Result<()> {
        let mut nodes = self.inner.nodes.write();
        let node = nodes.get_mut(&id).ok_or_else(|| Error::NotFound(format!("Node {id}")))?;
        node.labels.retain(|l| l != label);
        drop(nodes);
        let mut idx = self.inner.label_index.write();
        if let Some(ids) = idx.get_mut(label) {
            ids.retain(|nid| *nid != id);
        }
        Ok(())
    }

    // ========================================================================
    // Relationship CRUD
    // ========================================================================

    async fn create_relationship(
        &self,
        _tx: &mut MemoryTx,
        src: NodeId,
        dst: NodeId,
        rel_type: &str,
        props: PropertyMap,
    ) -> Result<RelId> {
        // Verify both nodes exist
        {
            let nodes = self.inner.nodes.read();
            if !nodes.contains_key(&src) {
                return Err(Error::NotFound(format!("Source node {src}")));
            }
            if !nodes.contains_key(&dst) {
                return Err(Error::NotFound(format!("Target node {dst}")));
            }
        }

        let id = RelId(self.inner.next_rel_id.fetch_add(1, Ordering::Relaxed));
        let rel = Relationship {
            id,
            element_id: None,
            src,
            dst,
            rel_type: rel_type.to_string(),
            properties: props,
        };

        self.inner.relationships.write().insert(id, rel);

        // Update adjacency for both endpoints
        let mut adj = self.inner.adjacency.write();
        adj.entry(src).or_default().push(id);
        if src != dst {
            adj.entry(dst).or_default().push(id);
        }

        Ok(id)
    }

    async fn get_relationship(&self, _tx: &MemoryTx, id: RelId) -> Result<Option<Relationship>> {
        Ok(self.inner.relationships.read().get(&id).cloned())
    }

    async fn set_relationship_property(
        &self,
        _tx: &mut MemoryTx,
        id: RelId,
        key: &str,
        val: Value,
    ) -> Result<()> {
        let mut rels = self.inner.relationships.write();
        let rel = rels.get_mut(&id)
            .ok_or_else(|| Error::NotFound(format!("Relationship {id}")))?;
        rel.properties.insert(key.to_string(), val);
        Ok(())
    }

    async fn remove_relationship_property(
        &self,
        _tx: &mut MemoryTx,
        id: RelId,
        key: &str,
    ) -> Result<()> {
        let mut rels = self.inner.relationships.write();
        let rel = rels.get_mut(&id)
            .ok_or_else(|| Error::NotFound(format!("Relationship {id}")))?;
        rel.properties.remove(key);
        Ok(())
    }

    async fn delete_relationship(&self, _tx: &mut MemoryTx, id: RelId) -> Result<bool> {
        let removed = self.inner.relationships.write().remove(&id);
        if let Some(rel) = &removed {
            let mut adj = self.inner.adjacency.write();
            if let Some(rels) = adj.get_mut(&rel.src) {
                rels.retain(|rid| *rid != id);
            }
            if rel.src != rel.dst {
                if let Some(rels) = adj.get_mut(&rel.dst) {
                    rels.retain(|rid| *rid != id);
                }
            }
        }
        Ok(removed.is_some())
    }

    // ========================================================================
    // Traversal
    // ========================================================================

    async fn get_relationships(
        &self,
        _tx: &MemoryTx,
        node: NodeId,
        dir: Direction,
        rel_type: Option<&str>,
    ) -> Result<Vec<Relationship>> {
        let adj = self.inner.adjacency.read();
        let rels = self.inner.relationships.read();

        let rel_ids = adj.get(&node).cloned().unwrap_or_default();
        let mut result = Vec::new();

        for rid in rel_ids {
            if let Some(rel) = rels.get(&rid) {
                // Direction filter
                let matches_dir = match dir {
                    Direction::Outgoing => rel.src == node,
                    Direction::Incoming => rel.dst == node,
                    Direction::Both => true,
                };
                // Type filter
                let matches_type = rel_type.map_or(true, |t| rel.rel_type == t);

                if matches_dir && matches_type {
                    result.push(rel.clone());
                }
            }
        }

        Ok(result)
    }

    async fn expand(
        &self,
        tx: &MemoryTx,
        node: NodeId,
        dir: Direction,
        rel_types: &[&str],
        depth: ExpandDepth,
    ) -> Result<Vec<Path>> {
        let (min_depth, max_depth) = match depth {
            ExpandDepth::Exact(d) => (d, d),
            ExpandDepth::Range { min, max } => (min, max),
            ExpandDepth::Unbounded => (1, 100), // safety limit
        };

        let mut results = Vec::new();
        let start_node = self.get_node(tx, node).await?
            .ok_or_else(|| Error::NotFound(format!("Node {node}")))?;

        // BFS expansion
        let mut queue: Vec<Path> = vec![Path::single(start_node)];

        for current_depth in 0..max_depth {
            let mut next_queue = Vec::new();

            for path in &queue {
                let tip = path.end();
                let rels = self.get_relationships(tx, tip.id, dir, None).await?;

                for rel in rels {
                    // Type filter
                    if !rel_types.is_empty() && !rel_types.contains(&rel.rel_type.as_str()) {
                        continue;
                    }

                    let next_id = rel.other_node(tip.id).unwrap_or(rel.dst);

                    // Avoid cycles
                    if path.nodes.iter().any(|n| n.id == next_id) {
                        continue;
                    }

                    if let Some(next_node) = self.get_node(tx, next_id).await? {
                        let mut new_path = path.clone();
                        new_path.append(rel, next_node);

                        if current_depth + 1 >= min_depth {
                            results.push(new_path.clone());
                        }
                        if current_depth + 1 < max_depth {
                            next_queue.push(new_path);
                        }
                    }
                }
            }

            queue = next_queue;
            if queue.is_empty() { break; }
        }

        Ok(results)
    }

    // ========================================================================
    // Index (stub for memory — label index is always maintained)
    // ========================================================================

    async fn create_index(&self, _label: &str, _property: &str, _index_type: IndexType) -> Result<()> {
        // No-op: memory backend always full-scans. No real indexes are maintained.
        Ok(())
    }

    async fn drop_index(&self, _label: &str, _property: &str) -> Result<()> {
        Ok(())
    }

    // ========================================================================
    // Schema introspection
    // ========================================================================

    async fn node_count(&self, _tx: &MemoryTx) -> Result<u64> {
        Ok(self.inner.nodes.read().len() as u64)
    }

    async fn relationship_count(&self, _tx: &MemoryTx) -> Result<u64> {
        Ok(self.inner.relationships.read().len() as u64)
    }

    async fn labels(&self, _tx: &MemoryTx) -> Result<Vec<String>> {
        Ok(self.inner.label_index.read().keys().cloned().collect())
    }

    async fn relationship_types(&self, _tx: &MemoryTx) -> Result<Vec<String>> {
        let rels = self.inner.relationships.read();
        let mut types: Vec<String> = rels.values().map(|r| r.rel_type.clone()).collect();
        types.sort();
        types.dedup();
        Ok(types)
    }

    // ========================================================================
    // Scan
    // ========================================================================

    async fn all_nodes(&self, _tx: &MemoryTx) -> Result<Vec<Node>> {
        Ok(self.inner.nodes.read().values().cloned().collect())
    }

    async fn nodes_by_label(&self, _tx: &MemoryTx, label: &str) -> Result<Vec<Node>> {
        let idx = self.inner.label_index.read();
        let nodes = self.inner.nodes.read();

        let ids = idx.get(label).cloned().unwrap_or_default();
        Ok(ids.iter().filter_map(|id| nodes.get(id).cloned()).collect())
    }

    async fn nodes_by_property(
        &self,
        _tx: &MemoryTx,
        label: &str,
        key: &str,
        value: &Value,
    ) -> Result<Vec<Node>> {
        // Brute force scan (memory backend doesn't have real property indexes)
        let idx = self.inner.label_index.read();
        let nodes = self.inner.nodes.read();

        let ids = idx.get(label).cloned().unwrap_or_default();
        Ok(ids.iter()
            .filter_map(|id| nodes.get(id))
            .filter(|n| n.get(key) == Some(value))
            .cloned()
            .collect())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_node() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let mut props = PropertyMap::new();
        props.insert("name".into(), Value::from("Ada"));

        let id = db.create_node(&mut tx, &["Person"], props).await.unwrap();
        let node = db.get_node(&tx, id).await.unwrap().unwrap();

        assert_eq!(node.labels, vec!["Person"]);
        assert_eq!(node.get("name"), Some(&Value::from("Ada")));
    }

    #[tokio::test]
    async fn test_create_relationship() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let a = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let b = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();

        let rel_id = db.create_relationship(&mut tx, a, b, "KNOWS", PropertyMap::new()).await.unwrap();
        let rel = db.get_relationship(&tx, rel_id).await.unwrap().unwrap();

        assert_eq!(rel.src, a);
        assert_eq!(rel.dst, b);
        assert_eq!(rel.rel_type, "KNOWS");
    }

    #[tokio::test]
    async fn test_cannot_delete_connected_node() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let a = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let b = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        db.create_relationship(&mut tx, a, b, "KNOWS", PropertyMap::new()).await.unwrap();

        let result = db.delete_node(&mut tx, a).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_all_nodes() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        db.create_node(&mut tx, &["Company"], PropertyMap::new()).await.unwrap();
        db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();

        let all = db.all_nodes(&tx).await.unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn test_detach_delete_node() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let a = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let b = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        db.create_relationship(&mut tx, a, b, "KNOWS", PropertyMap::new()).await.unwrap();

        // Normal delete should fail (has relationships)
        assert!(db.delete_node(&mut tx, a).await.is_err());

        // Detach delete should succeed
        assert!(db.detach_delete_node(&mut tx, a).await.unwrap());
        assert!(db.get_node(&tx, a).await.unwrap().is_none());
        assert_eq!(db.relationship_count(&tx).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_relationship_properties() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let a = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let b = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let rel_id = db.create_relationship(
            &mut tx, a, b, "KNOWS", PropertyMap::new(),
        ).await.unwrap();

        // Set property
        db.set_relationship_property(&mut tx, rel_id, "since", Value::from(2025i64)).await.unwrap();
        let rel = db.get_relationship(&tx, rel_id).await.unwrap().unwrap();
        assert_eq!(rel.properties.get("since"), Some(&Value::from(2025i64)));

        // Remove property
        db.remove_relationship_property(&mut tx, rel_id, "since").await.unwrap();
        let rel = db.get_relationship(&tx, rel_id).await.unwrap().unwrap();
        assert!(rel.properties.get("since").is_none());
    }

    #[tokio::test]
    async fn test_relationships_by_type() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let a = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let b = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let c = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();

        db.create_relationship(&mut tx, a, b, "KNOWS", PropertyMap::new()).await.unwrap();
        db.create_relationship(&mut tx, b, c, "WORKS_WITH", PropertyMap::new()).await.unwrap();
        db.create_relationship(&mut tx, a, c, "KNOWS", PropertyMap::new()).await.unwrap();

        let knows = db.relationships_by_type(&tx, "KNOWS").await.unwrap();
        assert_eq!(knows.len(), 2);

        let works = db.relationships_by_type(&tx, "WORKS_WITH").await.unwrap();
        assert_eq!(works.len(), 1);
    }

    #[tokio::test]
    async fn test_traversal() {
        let db = MemoryBackend::new();
        let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

        let a = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let b = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();
        let c = db.create_node(&mut tx, &["Person"], PropertyMap::new()).await.unwrap();

        db.create_relationship(&mut tx, a, b, "KNOWS", PropertyMap::new()).await.unwrap();
        db.create_relationship(&mut tx, b, c, "KNOWS", PropertyMap::new()).await.unwrap();

        let paths = db.expand(&tx, a, Direction::Outgoing, &["KNOWS"], ExpandDepth::Range { min: 1, max: 2 }).await.unwrap();

        // Should find a->b and a->b->c
        assert_eq!(paths.len(), 2);
    }
}
