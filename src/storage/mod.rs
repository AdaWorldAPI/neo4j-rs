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
}
