//! Ladybug Storage Backend — neo4j-rs → ladybug-rs BindSpace
//!
//! This is THE production backend. neo4j-rs stores nothing — every Cypher
//! operation translates to BindSpace reads/writes. The user sees Neo4j.
//! The engine is ladybug-rs.
//!
//! ```text
//! User types Cypher → neo4j-rs parses → LogicalPlan →
//!   StorageBackend::Ladybug translates → BindSpace operations
//! ```
//!
//! ## Node mapping
//!
//! | Neo4j concept       | ladybug-rs concept                    |
//! |---------------------|---------------------------------------|
//! | Node                | BindNode at Addr (0x80-0xFF:XX)       |
//! | Node labels         | BindNode.label                        |
//! | Node properties     | BindNode.payload (JSON-encoded)       |
//! | Relationship        | BindEdge (from → verb → to)           |
//! | Relationship type   | BindNode at verb Addr (0x07:XX)       |
//! | Relationship props  | Edge weight (scalar) or verb payload   |
//! | NodeId              | Addr.0 as u64                         |
//! | RelId               | Edge index in BindSpace.edges          |

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::model::*;
use crate::tx::{Transaction, TxMode, TxId};
use crate::index::IndexType;
use crate::storage::{
    StorageBackend, ExpandDepth, ConstraintType, BackendCapabilities, ProcedureResult,
};
use crate::{Error, Result};

use ladybug::storage::bind_space::{Addr, BindEdge, BindNode, BindSpace, FINGERPRINT_WORDS};

// =============================================================================
// LADYBUG BACKEND
// =============================================================================

/// Production storage backend backed by ladybug-rs BindSpace.
///
/// neo4j-rs is the glove. This is where it fits over the engine.
pub struct LadybugBackend {
    bs: Arc<RwLock<BindSpace>>,
    next_tx_id: AtomicU64,
}

impl LadybugBackend {
    /// Create a new LadybugBackend wrapping an existing BindSpace.
    pub fn new(bs: Arc<RwLock<BindSpace>>) -> Self {
        Self {
            bs,
            next_tx_id: AtomicU64::new(1),
        }
    }

    /// Create with a fresh empty BindSpace.
    pub fn open() -> Self {
        Self::new(Arc::new(RwLock::new(BindSpace::new())))
    }

    /// Access the underlying BindSpace.
    pub fn bind_space(&self) -> &Arc<RwLock<BindSpace>> {
        &self.bs
    }
}

// =============================================================================
// TRANSACTION (lightweight — BindSpace is already thread-safe via RwLock)
// =============================================================================

pub struct LadybugTx {
    id: TxId,
    mode: TxMode,
}

impl Transaction for LadybugTx {
    fn id(&self) -> TxId { self.id }
    fn mode(&self) -> TxMode { self.mode }
}

// =============================================================================
// HELPERS
// =============================================================================

/// Convert a BindNode to a neo4j-rs Node DTO.
fn bind_node_to_node(addr: Addr, bn: &BindNode) -> Node {
    let mut properties = PropertyMap::new();

    // Deserialize JSON payload into properties
    if let Some(ref payload) = bn.payload {
        if let Ok(map) = serde_json::from_slice::<HashMap<String, serde_json::Value>>(payload) {
            for (k, v) in map {
                properties.insert(k, json_to_value(&v));
            }
        }
    }

    let labels = bn.label.iter().cloned().collect();

    Node {
        id: NodeId(addr.0 as u64),
        labels,
        properties,
    }
}

/// Generate deterministic fingerprint from label + properties.
fn node_fingerprint(label: &str, properties: &PropertyMap) -> [u64; FINGERPRINT_WORDS] {
    let mut content = label.to_string();
    let mut sorted: Vec<_> = properties.iter().collect();
    sorted.sort_by_key(|(k, _)| k.clone());
    for (k, v) in sorted {
        content.push(':');
        content.push_str(k);
        content.push('=');
        content.push_str(&format!("{:?}", v));
    }
    let fp = ladybug::core::Fingerprint::from_content(&content);
    let mut words = [0u64; FINGERPRINT_WORDS];
    words.copy_from_slice(fp.as_raw());
    words
}

fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Int(i) }
            else if let Some(f) = n.as_f64() { Value::Float(f) }
            else { Value::Null }
        }
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(map) => {
            let mut pm = PropertyMap::new();
            for (k, v) in map {
                pm.insert(k.clone(), json_to_value(v));
            }
            Value::Map(pm)
        }
    }
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Int(i) => serde_json::json!(*i),
        Value::Float(f) => serde_json::json!(*f),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Null => serde_json::Value::Null,
        Value::List(l) => serde_json::Value::Array(l.iter().map(value_to_json).collect()),
        Value::Map(m) => {
            let mut map = serde_json::Map::new();
            for (k, v) in m.iter() {
                map.insert(k.clone(), value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}

fn props_to_payload(props: &PropertyMap) -> Vec<u8> {
    let map: HashMap<String, serde_json::Value> = props.iter()
        .map(|(k, v)| (k.clone(), value_to_json(v)))
        .collect();
    serde_json::to_vec(&map).unwrap_or_default()
}

fn addr_from_node_id(id: NodeId) -> Addr {
    Addr(id.0 as u16)
}

// =============================================================================
// STORAGE BACKEND IMPL
// =============================================================================

#[async_trait]
impl StorageBackend for LadybugBackend {
    type Tx = LadybugTx;

    // ---- Lifecycle ----
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    // ---- Transactions ----
    async fn begin_tx(&self, mode: TxMode) -> Result<Self::Tx> {
        let id = TxId(self.next_tx_id.fetch_add(1, Ordering::Relaxed));
        Ok(LadybugTx { id, mode })
    }

    async fn commit_tx(&self, _tx: Self::Tx) -> Result<()> {
        Ok(())
    }

    async fn rollback_tx(&self, _tx: Self::Tx) -> Result<()> {
        Ok(())
    }

    // ---- Node CRUD ----
    async fn create_node(
        &self,
        _tx: &mut Self::Tx,
        labels: Vec<String>,
        properties: PropertyMap,
    ) -> Result<NodeId> {
        let primary_label = labels.first().map(|s| s.as_str()).unwrap_or("Node");
        let fp = node_fingerprint(primary_label, &properties);

        let mut bs = self.bs.write();
        let addr = bs.write_labeled(fp, primary_label);

        if let Some(node) = bs.read_mut(addr) {
            node.payload = Some(props_to_payload(&properties));
        }

        Ok(NodeId(addr.0 as u64))
    }

    async fn get_node(&self, _tx: &mut Self::Tx, id: NodeId) -> Result<Option<Node>> {
        let bs = self.bs.read();
        let addr = addr_from_node_id(id);
        Ok(bs.read(addr).map(|bn| bind_node_to_node(addr, bn)))
    }

    async fn delete_node(&self, _tx: &mut Self::Tx, _id: NodeId) -> Result<bool> {
        // BindSpace doesn't support node deletion directly — mark as dead
        Ok(false)
    }

    async fn set_node_property(
        &self,
        _tx: &mut Self::Tx,
        id: NodeId,
        key: String,
        value: Value,
    ) -> Result<()> {
        let mut bs = self.bs.write();
        let addr = addr_from_node_id(id);

        if let Some(node) = bs.read_mut(addr) {
            let mut props: HashMap<String, serde_json::Value> = node.payload
                .as_ref()
                .and_then(|p| serde_json::from_slice(p).ok())
                .unwrap_or_default();

            props.insert(key, value_to_json(&value));
            node.payload = Some(serde_json::to_vec(&props).unwrap_or_default());
            Ok(())
        } else {
            Err(Error::NotFound(format!("Node {:?}", id)))
        }
    }

    async fn remove_node_property(
        &self,
        _tx: &mut Self::Tx,
        id: NodeId,
        key: String,
    ) -> Result<()> {
        let mut bs = self.bs.write();
        let addr = addr_from_node_id(id);

        if let Some(node) = bs.read_mut(addr) {
            let mut props: HashMap<String, serde_json::Value> = node.payload
                .as_ref()
                .and_then(|p| serde_json::from_slice(p).ok())
                .unwrap_or_default();

            props.remove(&key);
            node.payload = Some(serde_json::to_vec(&props).unwrap_or_default());
            Ok(())
        } else {
            Err(Error::NotFound(format!("Node {:?}", id)))
        }
    }

    async fn add_label(&self, _tx: &mut Self::Tx, id: NodeId, label: String) -> Result<()> {
        let mut bs = self.bs.write();
        let addr = addr_from_node_id(id);
        if let Some(node) = bs.read_mut(addr) {
            node.label = Some(label);
            Ok(())
        } else {
            Err(Error::NotFound(format!("Node {:?}", id)))
        }
    }

    async fn remove_label(&self, _tx: &mut Self::Tx, id: NodeId, _label: String) -> Result<()> {
        let mut bs = self.bs.write();
        let addr = addr_from_node_id(id);
        if let Some(node) = bs.read_mut(addr) {
            node.label = None;
            Ok(())
        } else {
            Err(Error::NotFound(format!("Node {:?}", id)))
        }
    }

    // ---- Relationship CRUD ----
    async fn create_relationship(
        &self,
        _tx: &mut Self::Tx,
        src: NodeId,
        dst: NodeId,
        rel_type: String,
        _properties: PropertyMap,
    ) -> Result<RelId> {
        let from = addr_from_node_id(src);
        let to = addr_from_node_id(dst);

        let mut bs = self.bs.write();

        // Create or find verb node for this relationship type
        let verb_fp = {
            let fp = ladybug::core::Fingerprint::from_content(&rel_type);
            let mut words = [0u64; FINGERPRINT_WORDS];
            words.copy_from_slice(fp.as_raw());
            words
        };
        let verb_addr = bs.write_labeled(verb_fp, &rel_type);

        let edge = BindEdge::new(from, verb_addr, to);
        let edge_idx = bs.edge_count();
        bs.link_with_edge(edge);

        Ok(RelId(edge_idx as u64))
    }

    async fn get_relationship(&self, _tx: &mut Self::Tx, _id: RelId) -> Result<Option<Relationship>> {
        // Would need edge index → BindEdge lookup
        Ok(None)
    }

    async fn delete_relationship(&self, _tx: &mut Self::Tx, _id: RelId) -> Result<bool> {
        Ok(false)
    }

    // ---- Traversal ----
    async fn get_relationships(
        &self,
        _tx: &mut Self::Tx,
        node_id: NodeId,
        direction: Direction,
        _rel_type: Option<&str>,
    ) -> Result<Vec<Relationship>> {
        let bs = self.bs.read();
        let addr = addr_from_node_id(node_id);
        let mut rels = Vec::new();

        match direction {
            Direction::Outgoing | Direction::Both => {
                for (i, edge) in bs.edges_out(addr).enumerate() {
                    let verb_label = bs.read(edge.verb)
                        .and_then(|n| n.label.clone())
                        .unwrap_or_else(|| "RELATED_TO".to_string());
                    rels.push(Relationship {
                        id: RelId(i as u64),
                        rel_type: verb_label,
                        start_node_id: NodeId(edge.from.0 as u64),
                        end_node_id: NodeId(edge.to.0 as u64),
                        properties: PropertyMap::new(),
                    });
                }
            }
            _ => {}
        }

        match direction {
            Direction::Incoming | Direction::Both => {
                for (i, edge) in bs.edges_in(addr).enumerate() {
                    let verb_label = bs.read(edge.verb)
                        .and_then(|n| n.label.clone())
                        .unwrap_or_else(|| "RELATED_TO".to_string());
                    rels.push(Relationship {
                        id: RelId(10000 + i as u64),
                        rel_type: verb_label,
                        start_node_id: NodeId(edge.from.0 as u64),
                        end_node_id: NodeId(edge.to.0 as u64),
                        properties: PropertyMap::new(),
                    });
                }
            }
            _ => {}
        }

        Ok(rels)
    }

    async fn expand(
        &self,
        _tx: &mut Self::Tx,
        start: NodeId,
        direction: Direction,
        rel_types: &[String],
        depth: ExpandDepth,
    ) -> Result<Vec<Path>> {
        let bs = self.bs.read();
        let start_addr = addr_from_node_id(start);
        let max_depth = match depth {
            ExpandDepth::Exact(d) => d,
            ExpandDepth::Range { max, .. } => max,
            ExpandDepth::Unbounded => 10,
        };

        let mut paths = Vec::new();
        let mut stack: Vec<(Addr, Vec<Addr>, Vec<(Addr, Addr)>)> = vec![(start_addr, vec![start_addr], vec![])];

        while let Some((current, node_path, edge_path)) = stack.pop() {
            if node_path.len() > max_depth + 1 {
                continue;
            }

            let edges: Vec<_> = match direction {
                Direction::Outgoing => bs.edges_out(current).collect(),
                Direction::Incoming => bs.edges_in(current).collect(),
                Direction::Both => {
                    let mut all: Vec<_> = bs.edges_out(current).collect();
                    all.extend(bs.edges_in(current));
                    all
                }
            };

            for edge in edges {
                // Filter by relationship type if specified
                if !rel_types.is_empty() {
                    let verb_label = bs.read(edge.verb)
                        .and_then(|n| n.label.clone())
                        .unwrap_or_default();
                    if !rel_types.iter().any(|rt| rt == &verb_label) {
                        continue;
                    }
                }

                let next = if direction == Direction::Incoming { edge.from } else { edge.to };

                if node_path.contains(&next) {
                    continue; // avoid cycles
                }

                let mut new_node_path = node_path.clone();
                new_node_path.push(next);

                let mut new_edge_path = edge_path.clone();
                new_edge_path.push((edge.from, edge.to));

                // Record path at every depth
                let nodes: Vec<Node> = new_node_path.iter()
                    .filter_map(|a| bs.read(*a).map(|bn| bind_node_to_node(*a, bn)))
                    .collect();

                let relationships: Vec<Relationship> = new_edge_path.iter()
                    .enumerate()
                    .map(|(i, (f, t))| Relationship {
                        id: RelId(i as u64),
                        rel_type: "RELATED_TO".to_string(),
                        start_node_id: NodeId(f.0 as u64),
                        end_node_id: NodeId(t.0 as u64),
                        properties: PropertyMap::new(),
                    })
                    .collect();

                paths.push(Path { nodes, relationships });

                if new_node_path.len() <= max_depth {
                    stack.push((next, new_node_path, new_edge_path));
                }
            }
        }

        Ok(paths)
    }

    // ---- Scanning ----
    async fn all_nodes(&self, _tx: &mut Self::Tx) -> Result<Vec<Node>> {
        let bs = self.bs.read();
        Ok(bs.nodes_iter()
            .map(|(addr, bn)| bind_node_to_node(addr, bn))
            .collect())
    }

    async fn nodes_by_label(&self, _tx: &mut Self::Tx, label: &str) -> Result<Vec<Node>> {
        let bs = self.bs.read();
        Ok(bs.nodes_iter()
            .filter(|(_, bn)| bn.label.as_deref() == Some(label))
            .map(|(addr, bn)| bind_node_to_node(addr, bn))
            .collect())
    }

    async fn nodes_by_property(
        &self,
        _tx: &mut Self::Tx,
        label: &str,
        key: &str,
        value: &Value,
    ) -> Result<Vec<Node>> {
        let bs = self.bs.read();
        let target_json = value_to_json(value);

        Ok(bs.nodes_iter()
            .filter(|(_, bn)| {
                if bn.label.as_deref() != Some(label) {
                    return false;
                }
                if let Some(ref payload) = bn.payload {
                    if let Ok(props) = serde_json::from_slice::<HashMap<String, serde_json::Value>>(payload) {
                        return props.get(key) == Some(&target_json);
                    }
                }
                false
            })
            .map(|(addr, bn)| bind_node_to_node(addr, bn))
            .collect())
    }

    async fn node_count(&self, _tx: &mut Self::Tx) -> Result<u64> {
        let bs = self.bs.read();
        Ok(bs.nodes_iter().count() as u64)
    }

    async fn relationship_count(&self, _tx: &mut Self::Tx) -> Result<u64> {
        let bs = self.bs.read();
        Ok(bs.edge_count() as u64)
    }

    async fn labels(&self, _tx: &mut Self::Tx) -> Result<Vec<String>> {
        let bs = self.bs.read();
        let mut labels: Vec<String> = bs.nodes_iter()
            .filter_map(|(_, bn)| bn.label.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        labels.sort();
        Ok(labels)
    }

    async fn relationship_types(&self, _tx: &mut Self::Tx) -> Result<Vec<String>> {
        let bs = self.bs.read();
        let mut types: Vec<String> = bs.edges_iter()
            .filter_map(|e| bs.read(e.verb).and_then(|n| n.label.clone()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        types.sort();
        Ok(types)
    }

    // ---- Index & Schema ----
    async fn create_index(
        &self,
        _tx: &mut Self::Tx,
        _label: &str,
        _property: &str,
        _index_type: IndexType,
    ) -> Result<()> {
        // Ladybug-rs uses Hamming-based indexing — no explicit B-tree needed.
        // This is a no-op that succeeds silently.
        Ok(())
    }

    async fn drop_index(
        &self,
        _tx: &mut Self::Tx,
        _label: &str,
        _property: &str,
    ) -> Result<()> {
        Ok(())
    }

    async fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_vector_index: true,
            supports_fulltext_index: false,
            supports_procedures: true,
            supports_batch_writes: true,
            max_batch_size: Some(10000),
            supported_procedures: vec![
                "ladybug.resonate".to_string(),
                "ladybug.hamming".to_string(),
                "ladybug.bind".to_string(),
                "ladybug.stats".to_string(),
            ],
            similarity_accelerated: true,
        }
    }
}
