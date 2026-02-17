//! LadybugBackend — StorageBackend backed by 8192-bit fingerprint containers.
//!
//! Connects the dots between three plans:
//! - INTEGRATION_ROADMAP.md Phase 4 (LadybugBackend struct + trait impl)
//! - CAM_CYPHER_REFERENCE.md (StorageBackend → CAM address routing)
//! - INTEGRATION_PLAN_SCHEMA_CHANGES.md rev 2 (BindSpace as storage)
//!
//! Every node gets fingerprinted into an 8192-bit container. Properties are
//! stored side-by-side for RETURN projections. Relationships are XOR-bound
//! (src ⊕ verb ⊕ dst). CALL ladybug.* procedures dispatch to NARS, spine,
//! DN tree, and resonance search.

pub mod fingerprint;
pub mod procedures;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use async_trait::async_trait;

use crate::model::*;
use crate::storage::{
    BackendCapabilities, ConstraintType, ExpandDepth, ProcedureResult, StorageBackend,
};
use crate::tx::{Transaction, TxId, TxMode};
use crate::index::IndexType;
use crate::{Error, Result};

use self::fingerprint::{ContainerDto, PropertyFingerprinter, bind_labels};

// ============================================================================
// Transaction
// ============================================================================

#[derive(Debug)]
pub struct LadybugTx {
    id: TxId,
    mode: TxMode,
}

impl Transaction for LadybugTx {
    fn id(&self) -> TxId { self.id }
    fn mode(&self) -> TxMode { self.mode }
}

// ============================================================================
// LadybugBackend
// ============================================================================

pub struct LadybugBackend {
    // Node storage
    id_to_slot: RwLock<HashMap<NodeId, usize>>,
    slot_to_id: RwLock<HashMap<usize, NodeId>>,
    fingerprints: RwLock<Vec<ContainerDto>>,
    node_props: RwLock<HashMap<NodeId, PropertyMap>>,
    node_labels: RwLock<HashMap<NodeId, Vec<String>>>,
    label_index: RwLock<HashMap<String, Vec<NodeId>>>,

    // Relationship storage
    rel_data: RwLock<HashMap<RelId, RelRecord>>,
    out_edges: RwLock<HashMap<NodeId, Vec<RelId>>>,
    in_edges: RwLock<HashMap<NodeId, Vec<RelId>>>,

    // Counters
    next_node_id: AtomicU64,
    next_rel_id: AtomicU64,
    next_tx_id: AtomicU64,

    // Tools
    fingerprinter: PropertyFingerprinter,
}

#[derive(Debug, Clone)]
struct RelRecord {
    src: NodeId,
    dst: NodeId,
    rel_type: String,
    props: PropertyMap,
    fingerprint: ContainerDto,
}

impl LadybugBackend {
    pub fn new() -> Self {
        Self {
            id_to_slot: RwLock::new(HashMap::new()),
            slot_to_id: RwLock::new(HashMap::new()),
            fingerprints: RwLock::new(Vec::new()),
            node_props: RwLock::new(HashMap::new()),
            node_labels: RwLock::new(HashMap::new()),
            label_index: RwLock::new(HashMap::new()),
            rel_data: RwLock::new(HashMap::new()),
            out_edges: RwLock::new(HashMap::new()),
            in_edges: RwLock::new(HashMap::new()),
            next_node_id: AtomicU64::new(1),
            next_rel_id: AtomicU64::new(1),
            next_tx_id: AtomicU64::new(1),
            fingerprinter: PropertyFingerprinter::cam(),
        }
    }

    fn rel_from_record(&self, id: RelId, rec: &RelRecord) -> Relationship {
        Relationship {
            id,
            element_id: None,
            src: rec.src,
            dst: rec.dst,
            rel_type: rec.rel_type.clone(),
            properties: rec.props.clone(),
        }
    }

    /// Hamming similarity search over all node fingerprints.
    pub fn similarity_search(&self, query: &ContainerDto, k: usize) -> Vec<(NodeId, f32)> {
        let fps = self.fingerprints.read().unwrap();
        let slot_map = self.slot_to_id.read().unwrap();

        let mut scored: Vec<(NodeId, f32)> = fps.iter()
            .enumerate()
            .filter_map(|(slot, fp)| {
                let id = slot_map.get(&slot)?;
                Some((*id, query.similarity(fp)))
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        scored
    }
}

impl Default for LadybugBackend {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// StorageBackend Implementation
// ============================================================================

#[async_trait]
impl StorageBackend for LadybugBackend {
    type Tx = LadybugTx;

    async fn shutdown(&self) -> Result<()> { Ok(()) }

    async fn begin_tx(&self, mode: TxMode) -> Result<LadybugTx> {
        Ok(LadybugTx {
            id: TxId(self.next_tx_id.fetch_add(1, Ordering::Relaxed)),
            mode,
        })
    }

    async fn commit_tx(&self, _tx: LadybugTx) -> Result<()> { Ok(()) }
    async fn rollback_tx(&self, _tx: LadybugTx) -> Result<()> { Ok(()) }

    async fn create_node(&self, _tx: &mut LadybugTx, labels: &[&str], props: PropertyMap) -> Result<NodeId> {
        let id = NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed));
        let fp = self.fingerprinter.fingerprint(&props);
        let _label_hash = bind_labels(&labels.iter().map(|s| s.to_string()).collect::<Vec<_>>());

        let slot = {
            let mut fps = self.fingerprints.write().unwrap();
            let s = fps.len();
            fps.push(fp);
            s
        };

        self.id_to_slot.write().unwrap().insert(id, slot);
        self.slot_to_id.write().unwrap().insert(slot, id);
        self.node_props.write().unwrap().insert(id, props);

        let label_strings: Vec<String> = labels.iter().map(|l| l.to_string()).collect();
        self.node_labels.write().unwrap().insert(id, label_strings.clone());

        {
            let mut idx = self.label_index.write().unwrap();
            for label in &label_strings {
                idx.entry(label.clone()).or_default().push(id);
            }
        }

        self.out_edges.write().unwrap().entry(id).or_default();
        self.in_edges.write().unwrap().entry(id).or_default();

        Ok(id)
    }

    async fn get_node(&self, _tx: &LadybugTx, id: NodeId) -> Result<Option<Node>> {
        let labels = self.node_labels.read().unwrap();
        let props = self.node_props.read().unwrap();
        if !labels.contains_key(&id) { return Ok(None); }
        Ok(Some(Node {
            id, element_id: None,
            labels: labels.get(&id).cloned().unwrap_or_default(),
            properties: props.get(&id).cloned().unwrap_or_default(),
        }))
    }

    async fn delete_node(&self, _tx: &mut LadybugTx, id: NodeId) -> Result<bool> {
        {
            let out = self.out_edges.read().unwrap();
            let inc = self.in_edges.read().unwrap();
            if out.get(&id).map_or(false, |v| !v.is_empty())
                || inc.get(&id).map_or(false, |v| !v.is_empty())
            {
                return Err(Error::ExecutionError(
                    "Cannot delete node with existing relationships. Use DETACH DELETE.".into(),
                ));
            }
        }
        let existed = self.node_labels.write().unwrap().remove(&id).is_some();
        if !existed { return Ok(false); }
        self.node_props.write().unwrap().remove(&id);
        self.out_edges.write().unwrap().remove(&id);
        self.in_edges.write().unwrap().remove(&id);
        let mut idx = self.label_index.write().unwrap();
        for ids in idx.values_mut() { ids.retain(|&nid| nid != id); }
        Ok(true)
    }

    async fn set_node_property(&self, _tx: &mut LadybugTx, id: NodeId, key: &str, val: Value) -> Result<()> {
        let mut props = self.node_props.write().unwrap();
        let map = props.get_mut(&id).ok_or_else(|| Error::ExecutionError(format!("Node {} not found", id.0)))?;
        map.insert(key.to_string(), val);
        let new_fp = self.fingerprinter.fingerprint(map);
        if let Some(&slot) = self.id_to_slot.read().unwrap().get(&id) {
            self.fingerprints.write().unwrap()[slot] = new_fp;
        }
        Ok(())
    }

    async fn remove_node_property(&self, _tx: &mut LadybugTx, id: NodeId, key: &str) -> Result<()> {
        let mut props = self.node_props.write().unwrap();
        if let Some(map) = props.get_mut(&id) {
            map.remove(key);
            let new_fp = self.fingerprinter.fingerprint(map);
            if let Some(&slot) = self.id_to_slot.read().unwrap().get(&id) {
                self.fingerprints.write().unwrap()[slot] = new_fp;
            }
        }
        Ok(())
    }

    async fn add_label(&self, _tx: &mut LadybugTx, id: NodeId, label: &str) -> Result<()> {
        let mut labels = self.node_labels.write().unwrap();
        if let Some(l) = labels.get_mut(&id) {
            if !l.contains(&label.to_string()) {
                l.push(label.to_string());
                self.label_index.write().unwrap().entry(label.to_string()).or_default().push(id);
            }
        }
        Ok(())
    }

    async fn remove_label(&self, _tx: &mut LadybugTx, id: NodeId, label: &str) -> Result<()> {
        let mut labels = self.node_labels.write().unwrap();
        if let Some(l) = labels.get_mut(&id) {
            l.retain(|s| s != label);
            if let Some(ids) = self.label_index.write().unwrap().get_mut(label) {
                ids.retain(|&nid| nid != id);
            }
        }
        Ok(())
    }

    async fn create_relationship(&self, _tx: &mut LadybugTx, src: NodeId, dst: NodeId, rel_type: &str, props: PropertyMap) -> Result<RelId> {
        {
            let labels = self.node_labels.read().unwrap();
            if !labels.contains_key(&src) { return Err(Error::ExecutionError(format!("Source node {} not found", src.0))); }
            if !labels.contains_key(&dst) { return Err(Error::ExecutionError(format!("Target node {} not found", dst.0))); }
        }
        let id = RelId(self.next_rel_id.fetch_add(1, Ordering::Relaxed));
        let verb_fp = PropertyFingerprinter::fingerprint_string(rel_type);
        let fps = self.fingerprints.read().unwrap();
        let src_slot = self.id_to_slot.read().unwrap().get(&src).copied();
        let dst_slot = self.id_to_slot.read().unwrap().get(&dst).copied();
        let rel_fp = match (src_slot, dst_slot) {
            (Some(s), Some(d)) => fps[s].xor(&verb_fp).xor(&fps[d]),
            _ => verb_fp,
        };
        drop(fps);
        self.rel_data.write().unwrap().insert(id, RelRecord { src, dst, rel_type: rel_type.to_string(), props, fingerprint: rel_fp });
        self.out_edges.write().unwrap().entry(src).or_default().push(id);
        self.in_edges.write().unwrap().entry(dst).or_default().push(id);
        Ok(id)
    }

    async fn get_relationship(&self, _tx: &LadybugTx, id: RelId) -> Result<Option<Relationship>> {
        Ok(self.rel_data.read().unwrap().get(&id).map(|rec| self.rel_from_record(id, rec)))
    }

    async fn delete_relationship(&self, _tx: &mut LadybugTx, id: RelId) -> Result<bool> {
        let rec = self.rel_data.write().unwrap().remove(&id);
        if let Some(rec) = rec {
            self.out_edges.write().unwrap().get_mut(&rec.src).map(|v| v.retain(|&r| r != id));
            self.in_edges.write().unwrap().get_mut(&rec.dst).map(|v| v.retain(|&r| r != id));
            Ok(true)
        } else { Ok(false) }
    }

    async fn set_relationship_property(&self, _tx: &mut LadybugTx, id: RelId, key: &str, val: Value) -> Result<()> {
        let mut data = self.rel_data.write().unwrap();
        data.get_mut(&id).map(|rec| rec.props.insert(key.to_string(), val))
            .ok_or_else(|| Error::ExecutionError(format!("Relationship {} not found", id.0)))?;
        Ok(())
    }

    async fn remove_relationship_property(&self, _tx: &mut LadybugTx, id: RelId, key: &str) -> Result<()> {
        self.rel_data.write().unwrap().get_mut(&id).map(|rec| rec.props.remove(key));
        Ok(())
    }

    async fn get_relationships(&self, _tx: &LadybugTx, node: NodeId, dir: Direction, rel_type: Option<&str>) -> Result<Vec<Relationship>> {
        let data = self.rel_data.read().unwrap();
        let out = self.out_edges.read().unwrap();
        let inc = self.in_edges.read().unwrap();
        let mut result = Vec::new();

        let collect = |ids: &[RelId], result: &mut Vec<Relationship>| {
            for &id in ids {
                if let Some(rec) = data.get(&id) {
                    if rel_type.is_none() || rel_type == Some(rec.rel_type.as_str()) {
                        result.push(self.rel_from_record(id, rec));
                    }
                }
            }
        };

        if matches!(dir, Direction::Outgoing | Direction::Both) {
            if let Some(ids) = out.get(&node) { collect(ids, &mut result); }
        }
        if matches!(dir, Direction::Incoming | Direction::Both) {
            if let Some(ids) = inc.get(&node) { collect(ids, &mut result); }
        }
        Ok(result)
    }

    async fn expand(&self, tx: &LadybugTx, node: NodeId, dir: Direction, rel_types: &[&str], depth: ExpandDepth) -> Result<Vec<Path>> {
        let max_depth = match depth { ExpandDepth::Exact(d) | ExpandDepth::Range { max: d, .. } => d, ExpandDepth::Unbounded => 10 };
        let min_depth = match depth { ExpandDepth::Range { min, .. } => min, _ => 1 };
        let mut paths = Vec::new();
        let mut queue: Vec<(Vec<NodeId>, Vec<Relationship>)> = vec![(vec![node], vec![])];

        while let Some((path_nodes, path_rels)) = queue.pop() {
            let current = *path_nodes.last().unwrap();
            if path_rels.len() >= min_depth {
                let labels = self.node_labels.read().unwrap();
                let props = self.node_props.read().unwrap();
                paths.push(Path {
                    nodes: path_nodes.iter().map(|&id| Node {
                        id, element_id: None,
                        labels: labels.get(&id).cloned().unwrap_or_default(),
                        properties: props.get(&id).cloned().unwrap_or_default(),
                    }).collect(),
                    relationships: path_rels.clone(),
                });
            }
            if path_rels.len() < max_depth {
                let rels = self.get_relationships(tx, current, dir, None).await?;
                for rel in rels {
                    if !rel_types.is_empty() && !rel_types.contains(&rel.rel_type.as_str()) { continue; }
                    let next = if rel.src == current { rel.dst } else { rel.src };
                    if path_nodes.contains(&next) { continue; }
                    let mut nn = path_nodes.clone(); nn.push(next);
                    let mut nr = path_rels.clone(); nr.push(rel);
                    queue.push((nn, nr));
                }
            }
        }
        Ok(paths)
    }

    async fn create_index(&self, _label: &str, _property: &str, _index_type: IndexType) -> Result<()> { Ok(()) }
    async fn drop_index(&self, _label: &str, _property: &str) -> Result<()> { Ok(()) }

    async fn node_count(&self, _tx: &LadybugTx) -> Result<u64> { Ok(self.node_labels.read().unwrap().len() as u64) }
    async fn relationship_count(&self, _tx: &LadybugTx) -> Result<u64> { Ok(self.rel_data.read().unwrap().len() as u64) }

    async fn labels(&self, _tx: &LadybugTx) -> Result<Vec<String>> {
        let idx = self.label_index.read().unwrap();
        Ok(idx.keys().filter(|k| idx.get(*k).map_or(false, |v| !v.is_empty())).cloned().collect())
    }

    async fn relationship_types(&self, _tx: &LadybugTx) -> Result<Vec<String>> {
        let mut types: Vec<String> = self.rel_data.read().unwrap().values().map(|r| r.rel_type.clone()).collect();
        types.sort(); types.dedup(); Ok(types)
    }

    async fn all_nodes(&self, _tx: &LadybugTx) -> Result<Vec<Node>> {
        let labels = self.node_labels.read().unwrap();
        let props = self.node_props.read().unwrap();
        Ok(labels.iter().map(|(&id, l)| Node { id, element_id: None, labels: l.clone(), properties: props.get(&id).cloned().unwrap_or_default() }).collect())
    }

    async fn nodes_by_label(&self, _tx: &LadybugTx, label: &str) -> Result<Vec<Node>> {
        let idx = self.label_index.read().unwrap();
        let labels = self.node_labels.read().unwrap();
        let props = self.node_props.read().unwrap();
        let ids = match idx.get(label) { Some(ids) => ids.clone(), None => return Ok(Vec::new()) };
        Ok(ids.into_iter().filter_map(|id| Some(Node { id, element_id: None, labels: labels.get(&id)?.clone(), properties: props.get(&id).cloned().unwrap_or_default() })).collect())
    }

    async fn nodes_by_property(&self, _tx: &LadybugTx, label: &str, key: &str, value: &Value) -> Result<Vec<Node>> {
        let idx = self.label_index.read().unwrap();
        let props = self.node_props.read().unwrap();
        let labels = self.node_labels.read().unwrap();
        let ids = match idx.get(label) { Some(ids) => ids.clone(), None => return Ok(Vec::new()) };
        Ok(ids.into_iter().filter_map(|id| {
            let p = props.get(&id)?;
            if p.get(key) == Some(value) { Some(Node { id, element_id: None, labels: labels.get(&id)?.clone(), properties: p.clone() }) } else { None }
        }).collect())
    }

    async fn create_constraint(&self, _label: &str, _property: &str, _ct: ConstraintType) -> Result<()> { Ok(()) }
    async fn drop_constraint(&self, _label: &str, _property: &str) -> Result<()> { Ok(()) }

    async fn call_procedure(&self, _tx: &LadybugTx, name: &str, args: Vec<Value>) -> Result<ProcedureResult> {
        let labels = self.node_labels.read().unwrap();
        let props = self.node_props.read().unwrap();
        let nodes: HashMap<NodeId, Node> = labels.iter().map(|(&id, l)| {
            (id, Node { id, element_id: None, labels: l.clone(), properties: props.get(&id).cloned().unwrap_or_default() })
        }).collect();
        procedures::dispatch(name, &args, &nodes)
    }

    async fn vector_query(&self, _tx: &LadybugTx, _index_name: &str, k: usize, query_vector: &[u8]) -> Result<Vec<(NodeId, f64)>> {
        let query = if query_vector.len() >= ContainerDto::BYTES {
            let mut words = [0u64; 128];
            for (i, chunk) in query_vector.chunks_exact(8).take(128).enumerate() {
                words[i] = u64::from_le_bytes(chunk.try_into().unwrap());
            }
            ContainerDto { words }
        } else {
            ContainerDto::random(fingerprint::bind_label_strs(&[std::str::from_utf8(query_vector).unwrap_or("")]))
        };
        Ok(self.similarity_search(&query, k).into_iter().map(|(id, s)| (id, s as f64)).collect())
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            supports_vector_index: true,
            supports_fulltext_index: false,
            supports_procedures: true,
            supports_batch_writes: true,
            max_batch_size: Some(10_000),
            supported_procedures: procedures::PROCEDURE_NAMES.iter().map(|s| s.to_string()).collect(),
            similarity_accelerated: true,
        }
    }
}
