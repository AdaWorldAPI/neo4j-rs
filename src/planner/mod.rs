//! Query planner — transforms Cypher AST into logical/physical plans.
//!
//! The planner is backend-agnostic. It produces logical operators that
//! the execution engine maps to StorageBackend calls.

use crate::model::PropertyMap;
use crate::cypher::ast::Statement;
use crate::{Error, Result};

/// Logical plan node.
#[derive(Debug, Clone)]
pub enum LogicalPlan {
    /// Scan all nodes with a given label
    NodeScan { label: String, alias: String },
    /// Index-backed property lookup
    IndexLookup { label: String, property: String, alias: String },
    /// Expand relationships from a node
    Expand { from: String, dir: crate::model::Direction, rel_types: Vec<String>, to: String, rel_alias: Option<String> },
    /// Filter rows by predicate
    Filter { input: Box<LogicalPlan>, predicate: crate::cypher::ast::Expr },
    /// Project columns
    Project { input: Box<LogicalPlan>, items: Vec<(crate::cypher::ast::Expr, String)> },
    /// Create node
    CreateNode { labels: Vec<String>, properties: Vec<(String, crate::cypher::ast::Expr)>, alias: String },
    /// Create relationship
    CreateRel { src: String, dst: String, rel_type: String, properties: Vec<(String, crate::cypher::ast::Expr)> },
    /// Limit output rows
    Limit { input: Box<LogicalPlan>, count: usize },
    /// Sort
    Sort { input: Box<LogicalPlan>, keys: Vec<(crate::cypher::ast::Expr, bool)> },
    /// Cartesian product of two inputs
    CartesianProduct { left: Box<LogicalPlan>, right: Box<LogicalPlan> },
    /// Empty leaf (produces one empty row)
    Argument,
}

/// Create a logical plan from a parsed AST.
pub fn plan(_ast: &Statement, _params: &PropertyMap) -> Result<LogicalPlan> {
    // TODO: Full planner implementation
    Err(Error::PlanError("Planner not yet implemented".into()))
}

/// Optimize a logical plan.
pub fn optimize(plan: LogicalPlan) -> Result<LogicalPlan> {
    // TODO: Cost-based optimizer
    // Rules: predicate pushdown, index selection, join ordering
    Ok(plan)
}
