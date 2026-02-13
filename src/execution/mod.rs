//! Query execution engine.
//!
//! Executes logical plans against a StorageBackend.

use std::collections::HashMap;
use crate::model::*;
use crate::storage::StorageBackend;
use crate::tx::Transaction;
use crate::planner::LogicalPlan;
use crate::{Error, Result};

/// Query execution result.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<ResultRow>,
    pub stats: ExecutionStats,
}

/// A single row in the result set.
#[derive(Debug, Clone)]
pub struct ResultRow {
    pub values: HashMap<String, Value>,
}

impl ResultRow {
    /// Get a typed value from the row.
    pub fn get<T: FromValue>(&self, key: &str) -> Result<T> {
        let val = self.values.get(key)
            .ok_or_else(|| Error::NotFound(format!("Column '{key}'")))?;
        T::from_value(val)
    }
}

/// Execution statistics.
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    pub nodes_created: u64,
    pub nodes_deleted: u64,
    pub relationships_created: u64,
    pub relationships_deleted: u64,
    pub properties_set: u64,
    pub labels_added: u64,
    pub labels_removed: u64,
    pub execution_time_ms: u64,
}

/// Convert from Value to concrete types.
pub trait FromValue: Sized {
    fn from_value(val: &Value) -> Result<Self>;
}

impl FromValue for Node {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::Node(n) => Ok(*n.clone()),
            _ => Err(Error::TypeError {
                expected: "Node".into(),
                got: val.type_name().into(),
            }),
        }
    }
}

impl FromValue for String {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::String(s) => Ok(s.clone()),
            _ => Err(Error::TypeError {
                expected: "String".into(),
                got: val.type_name().into(),
            }),
        }
    }
}

impl FromValue for i64 {
    fn from_value(val: &Value) -> Result<Self> {
        val.as_int().ok_or_else(|| Error::TypeError {
            expected: "Integer".into(),
            got: val.type_name().into(),
        })
    }
}

/// Execute a logical plan against a storage backend.
pub async fn execute<B: StorageBackend>(
    _backend: &B,
    _tx: &B::Tx,
    _plan: LogicalPlan,
) -> Result<QueryResult> {
    // TODO: Walk the plan tree, call StorageBackend methods, assemble results
    Err(Error::ExecutionError("Execution engine not yet implemented".into()))
}
