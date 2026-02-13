//! # neo4j-rs — Clean Rust Property Graph Database
//!
//! A zero-technical-debt reimplementation of Neo4j's graph model in Rust.
//!
//! ## Design Principles
//!
//! 1. **Trait-first**: `StorageBackend` is the contract between query engine and storage
//! 2. **Clean DTOs**: `Node`, `Relationship`, `Value` cross all boundaries
//! 3. **Parser owns nothing**: Cypher → AST is a pure function
//! 4. **Backend-agnostic planner**: logical plans don't know about storage
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use neo4j_rs::{Graph, Node, Value, PropertyMap};
//!
//! # async fn example() -> neo4j_rs::Result<()> {
//! // Connect to storage backend
//! let graph = Graph::open_memory().await?;
//!
//! // Execute Cypher
//! let mut params = PropertyMap::new();
//! params.insert("name".into(), Value::from("Ada"));
//! let result = graph.execute(
//!     "CREATE (n:Person {name: $name}) RETURN n",
//!     params,
//! ).await?;
//!
//! for row in &result.rows {
//!     println!("{:?}", row.get::<Node>("n")?);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Storage Backends
//!
//! | Backend | Feature | Description |
//! |---------|---------|-------------|
//! | Memory | (default) | In-memory graph for testing/embedding |
//! | Bolt | `bolt` | Connect to external Neo4j via Bolt protocol |
//! | Ladybug | `ladybug` | Hamming-accelerated via ladybug-rs + holograph |

// ============================================================================
// Modules
// ============================================================================

pub mod model;
pub mod cypher;
pub mod planner;
pub mod execution;
pub mod storage;
pub mod tx;
pub mod index;

// ============================================================================
// Re-exports: Model (the DTOs)
// ============================================================================

pub use model::{
    Node, Relationship, Path, Value, PropertyMap,
    NodeId, RelId, Direction,
};

// ============================================================================
// Re-exports: Storage
// ============================================================================

pub use storage::{
    StorageBackend, BackendConfig, ConstraintType,
    BackendCapabilities, ProcedureResult,
};

// ============================================================================
// Re-exports: Transactions
// ============================================================================

pub use tx::{Transaction, TxMode, TxId};

// ============================================================================
// Re-exports: Execution
// ============================================================================

pub use execution::{QueryResult, ResultRow};

// ============================================================================
// Top-level Graph handle
// ============================================================================

/// The primary entry point. A `Graph` wraps a storage backend and
/// provides Cypher execution.
pub struct Graph<B: StorageBackend> {
    backend: B,
    // Future: schema cache, index registry, prepared statement cache
}

impl<B: StorageBackend> Graph<B> {
    /// Create a Graph with the given backend.
    pub fn with_backend(backend: B) -> Self {
        Self { backend }
    }

    /// Execute a Cypher query with parameters.
    pub async fn execute<P>(&self, query: &str, params: P) -> Result<QueryResult>
    where
        P: Into<PropertyMap>,
    {
        // Phase 1: Parse
        let ast = cypher::parse(query)?;

        // Phase 2: Plan
        let logical = planner::plan(&ast, &params.into())?;

        // Phase 3: Optimize
        let optimized = planner::optimize(logical)?;

        // Phase 4: Execute
        let mut tx = self.backend.begin_tx(TxMode::ReadOnly).await?;
        let result = execution::execute(&self.backend, &mut tx, optimized).await?;
        self.backend.commit_tx(tx).await?;

        Ok(result)
    }

    /// Execute a write query (CREATE, MERGE, DELETE, SET, etc.)
    pub async fn mutate<P>(&self, query: &str, params: P) -> Result<QueryResult>
    where
        P: Into<PropertyMap>,
    {
        let ast = cypher::parse(query)?;
        let logical = planner::plan(&ast, &params.into())?;
        let optimized = planner::optimize(logical)?;

        let mut tx = self.backend.begin_tx(TxMode::ReadWrite).await?;
        let result = execution::execute(&self.backend, &mut tx, optimized).await?;
        self.backend.commit_tx(tx).await?;

        Ok(result)
    }

    /// Begin an explicit transaction.
    pub async fn begin(&self, mode: TxMode) -> Result<ExplicitTx<'_, B>> {
        let tx = self.backend.begin_tx(mode).await?;
        Ok(ExplicitTx { graph: self, tx: Some(tx) })
    }

    /// Access the underlying backend (for advanced use).
    pub fn backend(&self) -> &B {
        &self.backend
    }
}

/// In-memory graph for testing and embedding.
impl Graph<storage::MemoryBackend> {
    pub async fn open_memory() -> Result<Self> {
        let backend = storage::MemoryBackend::new();
        Ok(Self::with_backend(backend))
    }
}

/// Explicit transaction handle. Warns on drop without commit/rollback.
pub struct ExplicitTx<'g, B: StorageBackend> {
    graph: &'g Graph<B>,
    tx: Option<B::Tx>,
}

impl<'g, B: StorageBackend> ExplicitTx<'g, B> {
    pub async fn execute<P>(&mut self, query: &str, params: P) -> Result<QueryResult>
    where
        P: Into<PropertyMap>,
    {
        let ast = cypher::parse(query)?;
        let logical = planner::plan(&ast, &params.into())?;
        let optimized = planner::optimize(logical)?;
        let tx = self.tx.as_mut().ok_or_else(|| Error::TxError("Transaction already finished".into()))?;
        execution::execute(&self.graph.backend, tx, optimized).await
    }

    pub async fn commit(mut self) -> Result<()> {
        let tx = self.tx.take().ok_or_else(|| Error::TxError("Transaction already finished".into()))?;
        self.graph.backend.commit_tx(tx).await
    }

    pub async fn rollback(mut self) -> Result<()> {
        let tx = self.tx.take().ok_or_else(|| Error::TxError("Transaction already finished".into()))?;
        self.graph.backend.rollback_tx(tx).await
    }
}

impl<'g, B: StorageBackend> Drop for ExplicitTx<'g, B> {
    fn drop(&mut self) {
        if self.tx.is_some() {
            tracing::warn!(
                "ExplicitTx dropped without commit or rollback — transaction abandoned. \
                 Call .commit() or .rollback() explicitly."
            );
        }
    }
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cypher syntax error at position {position}: {message}")]
    SyntaxError { position: usize, message: String },

    #[error("Semantic error: {0}")]
    SemanticError(String),

    #[error("Type error: expected {expected}, got {got}")]
    TypeError { expected: String, got: String },

    #[error("Planning error: {0}")]
    PlanError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Transaction error: {0}")]
    TxError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
