//! Index management.

use serde::{Deserialize, Serialize};

/// Type of index to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree index for equality and range queries.
    BTree,
    /// Full-text search index.
    FullText,
    /// Unique constraint (implies B-tree).
    Unique,
    /// Vector similarity index (ladybug-rs extension).
    Vector,
}
