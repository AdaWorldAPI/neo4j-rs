//! # Storage Backend Trait
//!
//! This is THE contract between neo4j-rs and any storage engine.
//! Every operation a graph database needs is defined here.
//!
//! ## Implementations
//!
//! | Backend | Module | Description |
//! |---------|--------|-------------|
//! | `MemoryBackend` | `memory` | In-memory for testing/embedding |
//! | `BoltBackend` | `bolt` | External Neo4j via Bolt protocol |
//! | `LadybugBackend` | `ladybug` | Hamming-accelerated via ladybug-rs |

pub mod memory;
#[cfg(feature = "bolt")]
pub mod bolt;
#[cfg(feature = "ladybug")]
pub mod ladybug;

use async_trait::async_trait;
use crate::model::*;
use crate::tx::{Transaction, TxMode, TxId};
use crate::index::IndexType;
use crate::{Error, Result};

pub use memory::MemoryBackend;

// ============================================================================
// Backend Configuration
// ============================================================================

/// Configuration for connecting to a storage backend.
#[derive(Debug, Clone)]
pub enum BackendConfig {
    /// In-memory (no persistence)
    Memory,

    /// Neo4j Bolt protocol
    #[cfg(feature = "bolt")]
    Bolt {
        uri: String,
        user: String,
        password: String,
        database: Option<String>,
    },

    /// ladybug-rs local storage
    #[cfg(feature = "ladybug")]
    Ladybug {
        data_dir: std::path::PathBuf,
        cache_size_mb: usize,
    },
}

// ============================================================================
// Expand depth specification
// ============================================================================

/// Depth specification for graph expansion.
#[derive(Debug, Clone, Copy)]
pub enum ExpandDepth {
    /// Exact depth
    Exact(usize),
    /// Range: min..max (inclusive)
    Range { min: usize, max: usize },
    /// Unbounded (up to implementation limit)
    Unbounded,
}

// ============================================================================
// Constraint types
// ============================================================================

/// Type of constraint to create on a label+property pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintType {
    /// Property value must be unique for nodes with this label.
    Unique,
    /// Property must exist on all nodes with this label.
    Exists,
}

// ============================================================================
// Backend capabilities
// ============================================================================

/// What a backend can do — used by the planner for optimization decisions.
///
/// All fields default to false / empty. Backends override via `capabilities()`.
#[derive(Debug, Clone, Default)]
pub struct BackendCapabilities {
    pub supports_vector_index: bool,
    pub supports_fulltext_index: bool,
    pub supports_procedures: bool,
    pub supports_batch_writes: bool,
    pub max_batch_size: Option<usize>,
    pub supported_procedures: Vec<String>,
    pub similarity_accelerated: bool,
}

// ============================================================================
// Procedure result
// ============================================================================

/// Result of a procedure call or raw query execution.
///
/// This lightweight type lives in the storage layer so that `call_procedure()`
/// and `execute_raw()` can return structured results without importing from
/// the execution module.
#[derive(Debug, Clone, Default)]
pub struct ProcedureResult {
    pub columns: Vec<String>,
    pub rows: Vec<std::collections::HashMap<String, Value>>,
}

// ============================================================================
// StorageBackend Trait
// ============================================================================

/// The universal storage contract.
///
/// Any backend that implements this trait can serve as the storage layer
/// for neo4j-rs. The trait is intentionally broad — backends should return
/// `Error::ExecutionError("not supported")` for operations they can't handle
/// rather than having a hundred optional methods.
#[async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    /// The transaction type for this backend.
    type Tx: Transaction;

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Shut down the backend, flushing any pending writes.
    async fn shutdown(&self) -> Result<()>;

    // ========================================================================
    // Transactions
    // ========================================================================

    /// Begin a new transaction.
    async fn begin_tx(&self, mode: TxMode) -> Result<Self::Tx>;

    /// Commit a transaction.
    async fn commit_tx(&self, tx: Self::Tx) -> Result<()>;

    /// Roll back a transaction.
    async fn rollback_tx(&self, tx: Self::Tx) -> Result<()>;

    // ========================================================================
    // Node CRUD
    // ========================================================================

    /// Create a node with the given labels and properties.
    async fn create_node(
        &self,
        tx: &mut Self::Tx,
        labels: &[&str],
        props: PropertyMap,
    ) -> Result<NodeId>;

    /// Get a node by ID. Returns None if not found.
    async fn get_node(&self, tx: &Self::Tx, id: NodeId) -> Result<Option<Node>>;

    /// Delete a node. Returns true if it existed.
    /// Fails if the node still has relationships (Neo4j semantics).
    async fn delete_node(&self, tx: &mut Self::Tx, id: NodeId) -> Result<bool>;

    /// Set a property on a node (upsert).
    async fn set_node_property(
        &self,
        tx: &mut Self::Tx,
        id: NodeId,
        key: &str,
        val: Value,
    ) -> Result<()>;

    /// Remove a property from a node.
    async fn remove_node_property(
        &self,
        tx: &mut Self::Tx,
        id: NodeId,
        key: &str,
    ) -> Result<()>;

    /// Add a label to a node.
    async fn add_label(&self, tx: &mut Self::Tx, id: NodeId, label: &str) -> Result<()>;

    /// Remove a label from a node.
    async fn remove_label(&self, tx: &mut Self::Tx, id: NodeId, label: &str) -> Result<()>;

    /// Delete a node and all its relationships in one operation.
    /// Neo4j: `DETACH DELETE n`
    ///
    /// Default: get all relationships, delete each, then delete the node.
    async fn detach_delete_node(&self, tx: &mut Self::Tx, id: NodeId) -> Result<bool> {
        let rels = self.get_relationships(tx, id, Direction::Both, None).await?;
        for rel in &rels {
            self.delete_relationship(tx, rel.id).await?;
        }
        self.delete_node(tx, id).await
    }

    // ========================================================================
    // Relationship CRUD
    // ========================================================================

    /// Create a relationship between two nodes.
    async fn create_relationship(
        &self,
        tx: &mut Self::Tx,
        src: NodeId,
        dst: NodeId,
        rel_type: &str,
        props: PropertyMap,
    ) -> Result<RelId>;

    /// Get a relationship by ID.
    async fn get_relationship(&self, tx: &Self::Tx, id: RelId) -> Result<Option<Relationship>>;

    /// Delete a relationship. Returns true if it existed.
    async fn delete_relationship(&self, tx: &mut Self::Tx, id: RelId) -> Result<bool>;

    /// Set a property on a relationship (upsert).
    ///
    /// Default returns error — override for backends with relationship property CRUD.
    async fn set_relationship_property(
        &self,
        _tx: &mut Self::Tx,
        _id: RelId,
        _key: &str,
        _val: Value,
    ) -> Result<()> {
        Err(Error::ExecutionError("relationship property set not supported".into()))
    }

    /// Remove a property from a relationship.
    ///
    /// Default returns error — override for backends with relationship property CRUD.
    async fn remove_relationship_property(
        &self,
        _tx: &mut Self::Tx,
        _id: RelId,
        _key: &str,
    ) -> Result<()> {
        Err(Error::ExecutionError("relationship property remove not supported".into()))
    }

    // ========================================================================
    // Traversal
    // ========================================================================

    /// Get all relationships of a node, optionally filtered by direction and type.
    async fn get_relationships(
        &self,
        tx: &Self::Tx,
        node: NodeId,
        dir: Direction,
        rel_type: Option<&str>,
    ) -> Result<Vec<Relationship>>;

    /// Expand from a node: BFS/DFS traversal to the given depth.
    async fn expand(
        &self,
        tx: &Self::Tx,
        node: NodeId,
        dir: Direction,
        rel_types: &[&str],
        depth: ExpandDepth,
    ) -> Result<Vec<Path>>;

    // ========================================================================
    // Index
    // ========================================================================

    /// Create an index on a label+property combination.
    async fn create_index(
        &self,
        label: &str,
        property: &str,
        index_type: IndexType,
    ) -> Result<()>;

    /// Drop an index.
    async fn drop_index(&self, label: &str, property: &str) -> Result<()>;

    // ========================================================================
    // Schema introspection
    // ========================================================================

    /// Total number of nodes.
    async fn node_count(&self, tx: &Self::Tx) -> Result<u64>;

    /// Total number of relationships.
    async fn relationship_count(&self, tx: &Self::Tx) -> Result<u64>;

    /// All distinct labels in the graph.
    async fn labels(&self, tx: &Self::Tx) -> Result<Vec<String>>;

    /// All distinct relationship types in the graph.
    async fn relationship_types(&self, tx: &Self::Tx) -> Result<Vec<String>>;

    // ========================================================================
    // Scan
    // ========================================================================

    /// Return all nodes (no label filter).
    ///
    /// This is the one new required method with no default — every backend
    /// must implement it because "scan everything" can't be generalized.
    async fn all_nodes(&self, tx: &Self::Tx) -> Result<Vec<Node>>;

    /// Find all nodes with a given label.
    async fn nodes_by_label(&self, tx: &Self::Tx, label: &str) -> Result<Vec<Node>>;

    /// Find nodes by label + property value (index-backed if available).
    async fn nodes_by_property(
        &self,
        tx: &Self::Tx,
        label: &str,
        key: &str,
        value: &Value,
    ) -> Result<Vec<Node>>;

    /// Find all relationships of a given type.
    ///
    /// Default: scans all nodes and collects outgoing relationships of that type.
    async fn relationships_by_type(
        &self,
        tx: &Self::Tx,
        rel_type: &str,
    ) -> Result<Vec<Relationship>> {
        let mut result = Vec::new();
        let nodes = self.all_nodes(tx).await?;
        for node in &nodes {
            let rels = self.get_relationships(
                tx, node.id, Direction::Outgoing, Some(rel_type),
            ).await?;
            result.extend(rels);
        }
        Ok(result)
    }

    // ========================================================================
    // Constraints
    // ========================================================================

    /// Create a schema constraint. Neo4j: `CREATE CONSTRAINT ...`
    ///
    /// Default returns "not supported".
    async fn create_constraint(
        &self,
        _label: &str,
        _property: &str,
        _constraint_type: ConstraintType,
    ) -> Result<()> {
        Err(Error::ExecutionError("constraints not supported".into()))
    }

    /// Drop a schema constraint.
    ///
    /// Default returns "not supported".
    async fn drop_constraint(&self, _label: &str, _property: &str) -> Result<()> {
        Err(Error::ExecutionError("constraints not supported".into()))
    }

    // ========================================================================
    // Batch operations
    // ========================================================================

    /// Batch create nodes. Optimizable for columnar backends (e.g. Lance).
    ///
    /// Default falls back to sequential `create_node` calls.
    async fn create_nodes_batch(
        &self,
        tx: &mut Self::Tx,
        nodes: Vec<(Vec<String>, PropertyMap)>,
    ) -> Result<Vec<NodeId>> {
        let mut ids = Vec::with_capacity(nodes.len());
        for (labels, props) in nodes {
            let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            ids.push(self.create_node(tx, &label_refs, props).await?);
        }
        Ok(ids)
    }

    /// Batch create relationships.
    ///
    /// Default falls back to sequential `create_relationship` calls.
    async fn create_relationships_batch(
        &self,
        tx: &mut Self::Tx,
        rels: Vec<(NodeId, NodeId, String, PropertyMap)>,
    ) -> Result<Vec<RelId>> {
        let mut ids = Vec::with_capacity(rels.len());
        for (src, dst, rel_type, props) in rels {
            ids.push(self.create_relationship(tx, src, dst, &rel_type, props).await?);
        }
        Ok(ids)
    }

    // ========================================================================
    // Escape hatches
    // ========================================================================

    /// Pass-through for backend-native queries.
    ///
    /// Bolt: forwards the Cypher string to Neo4j.
    /// Ladybug: could translate to DataFusion SQL.
    /// Memory: not supported.
    async fn execute_raw(
        &self,
        _tx: &Self::Tx,
        _query: &str,
        _params: PropertyMap,
    ) -> Result<ProcedureResult> {
        Err(Error::ExecutionError("raw query execution not supported".into()))
    }

    /// Call a registered procedure. Neo4j: `CALL name(args) YIELD cols`.
    ///
    /// This is the standard extension point — APOC, GDS, and ladybug-rs
    /// cognitive operations all go through here.
    async fn call_procedure(
        &self,
        _tx: &Self::Tx,
        _name: &str,
        _args: Vec<Value>,
    ) -> Result<ProcedureResult> {
        Err(Error::ExecutionError("procedures not supported".into()))
    }

    /// Vector similarity search (Neo4j 5.x compatible).
    ///
    /// Returns (NodeId, score) pairs ordered by similarity.
    async fn vector_query(
        &self,
        _tx: &Self::Tx,
        _index_name: &str,
        _k: usize,
        _query_vector: &[u8],
    ) -> Result<Vec<(NodeId, f64)>> {
        Err(Error::ExecutionError("vector index not supported".into()))
    }

    // ========================================================================
    // Capability negotiation
    // ========================================================================

    /// Report what this backend can do.
    ///
    /// The planner uses this to choose optimization strategies. For example,
    /// if `similarity_accelerated` is true, the optimizer can push Hamming
    /// filters into the scan operator instead of post-filtering.
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::default()
    }
}
