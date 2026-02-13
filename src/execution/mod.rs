//! Query execution engine.
//!
//! Executes logical plans against a StorageBackend.

use std::collections::HashMap;
use crate::model::*;
use crate::cypher::ast::{Expr, Literal, BinaryOp, UnaryOp, StringOp};
use crate::storage::StorageBackend;
use crate::planner::LogicalPlan;
use crate::{Error, Result};

/// Query execution result.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<ResultRow>,
    pub stats: ExecutionStats,
}

/// A single row in the result set. Preserves column order.
#[derive(Debug, Clone)]
pub struct ResultRow {
    pub values: Vec<(String, Value)>,
}

impl ResultRow {
    /// Get a typed value from the row by column name.
    pub fn get<T: FromValue>(&self, key: &str) -> Result<T> {
        let val = self.values.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .ok_or_else(|| Error::NotFound(format!("Column '{key}'")))?;
        T::from_value(val)
    }

    /// Get a raw Value reference by column name.
    pub fn get_value(&self, key: &str) -> Option<&Value> {
        self.values.iter().find(|(k, _)| k == key).map(|(_, v)| v)
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

impl FromValue for f64 {
    fn from_value(val: &Value) -> Result<Self> {
        val.as_float().ok_or_else(|| Error::TypeError {
            expected: "Float".into(),
            got: val.type_name().into(),
        })
    }
}

impl FromValue for bool {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::Bool(b) => Ok(*b),
            _ => Err(Error::TypeError { expected: "Bool".into(), got: val.type_name().into() }),
        }
    }
}

impl FromValue for Value {
    fn from_value(val: &Value) -> Result<Self> {
        Ok(val.clone())
    }
}

impl FromValue for Relationship {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::Relationship(r) => Ok(*r.clone()),
            _ => Err(Error::TypeError { expected: "Relationship".into(), got: val.type_name().into() }),
        }
    }
}

impl FromValue for Path {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::Path(p) => Ok(*p.clone()),
            _ => Err(Error::TypeError { expected: "Path".into(), got: val.type_name().into() }),
        }
    }
}

impl FromValue for Vec<Value> {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::List(l) => Ok(l.clone()),
            _ => Err(Error::TypeError { expected: "List".into(), got: val.type_name().into() }),
        }
    }
}

impl FromValue for HashMap<String, Value> {
    fn from_value(val: &Value) -> Result<Self> {
        match val {
            Value::Map(m) => Ok(m.clone()),
            _ => Err(Error::TypeError { expected: "Map".into(), got: val.type_name().into() }),
        }
    }
}

/// Execute a logical plan against a storage backend.
///
/// Takes `&mut B::Tx` because write operations (CREATE, SET, DELETE) need
/// mutable transaction access. Read-only plans simply don't mutate it.
pub async fn execute<B: StorageBackend>(
    backend: &B,
    tx: &mut B::Tx,
    plan: LogicalPlan,
    params: PropertyMap,
) -> Result<QueryResult> {
    let mut ctx = ExecContext::with_params(params);
    let rows = execute_plan(backend, tx, &plan, &mut ctx).await?;

    let columns = ctx.columns.clone();
    let result_rows: Vec<ResultRow> = rows.into_iter().map(|row| {
        let values: Vec<(String, Value)> = columns.iter()
            .map(|col| (col.clone(), row.get(col).cloned().unwrap_or(Value::Null)))
            .collect();
        ResultRow { values }
    }).collect();

    Ok(QueryResult {
        columns,
        rows: result_rows,
        stats: ctx.stats,
    })
}

// ============================================================================
// Execution context
// ============================================================================

type Row = HashMap<String, Value>;

struct ExecContext {
    columns: Vec<String>,
    stats: ExecutionStats,
    params: PropertyMap,
}

impl ExecContext {
    fn with_params(params: PropertyMap) -> Self {
        Self {
            columns: Vec::new(),
            stats: ExecutionStats::default(),
            params,
        }
    }
}

// ============================================================================
// Plan executor (recursive walk over LogicalPlan tree)
// ============================================================================

fn execute_plan<'a, B: StorageBackend>(
    backend: &'a B,
    tx: &'a mut B::Tx,
    plan: &'a LogicalPlan,
    ctx: &'a mut ExecContext,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Row>>> + Send + 'a>> {
    Box::pin(async move {
    match plan {
        LogicalPlan::Argument => {
            // Produce one empty row (seed for pipelines)
            Ok(vec![HashMap::new()])
        }

        LogicalPlan::NodeScan { label, alias } => {
            let nodes = backend.nodes_by_label(tx, label).await?;
            let rows: Vec<Row> = nodes.into_iter().map(|n| {
                let mut row = HashMap::new();
                row.insert(alias.clone(), Value::Node(Box::new(n)));
                row
            }).collect();
            if !ctx.columns.contains(alias) {
                ctx.columns.push(alias.clone());
            }
            Ok(rows)
        }

        LogicalPlan::AllNodesScan { alias } => {
            let nodes = backend.all_nodes(tx).await?;
            let rows: Vec<Row> = nodes.into_iter().map(|n| {
                let mut row = HashMap::new();
                row.insert(alias.clone(), Value::Node(Box::new(n)));
                row
            }).collect();
            if !ctx.columns.contains(alias) {
                ctx.columns.push(alias.clone());
            }
            Ok(rows)
        }

        LogicalPlan::IndexLookup { label, property, alias } => {
            // Falls back to label scan — memory backend has no real indexes
            let nodes = backend.nodes_by_label(tx, label).await?;
            let rows: Vec<Row> = nodes.into_iter().map(|n| {
                let mut row = HashMap::new();
                row.insert(alias.clone(), Value::Node(Box::new(n)));
                row
            }).collect();
            if !ctx.columns.contains(alias) {
                ctx.columns.push(alias.clone());
            }
            let _ = property; // suppress warning
            Ok(rows)
        }

        LogicalPlan::Expand { input, from, dir, rel_types, to, rel_alias } => {
            // Execute input pipeline first to get rows with 'from' variable bound
            let input_rows = execute_plan(backend, tx, input, ctx).await?;
            let mut rows = Vec::new();
            for input_row in &input_rows {
                if let Some(Value::Node(from_node)) = input_row.get(from) {
                    let rels = backend.get_relationships(tx, from_node.id, *dir, None).await?;
                    for rel in rels {
                        if !rel_types.is_empty() && !rel_types.contains(&rel.rel_type) {
                            continue;
                        }
                        let other_id = if rel.src == from_node.id { rel.dst } else { rel.src };
                        if let Some(other) = backend.get_node(tx, other_id).await? {
                            let mut row = input_row.clone();
                            row.insert(to.clone(), Value::Node(Box::new(other)));
                            if let Some(ra) = rel_alias {
                                row.insert(ra.clone(), Value::Relationship(Box::new(rel.clone())));
                            }
                            rows.push(row);
                        }
                    }
                }
            }
            for col in [from, to] {
                if !ctx.columns.contains(col) {
                    ctx.columns.push(col.clone());
                }
            }
            if let Some(ra) = rel_alias {
                if !ctx.columns.contains(ra) {
                    ctx.columns.push(ra.clone());
                }
            }
            Ok(rows)
        }

        LogicalPlan::Filter { input, predicate } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            let mut filtered = Vec::new();
            for row in rows {
                let val = eval_expr(predicate, &row, &ctx.params)?;
                if val.is_truthy() {
                    filtered.push(row);
                }
            }
            Ok(filtered)
        }

        LogicalPlan::Project { input, items } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            // Reset columns to the projection list
            ctx.columns = items.iter().map(|(_, alias)| alias.clone()).collect();

            let mut projected = Vec::new();
            for row in &rows {
                let mut new_row = HashMap::new();
                for (expr, alias) in items {
                    let val = eval_expr(expr, row, &ctx.params)?;
                    new_row.insert(alias.clone(), val);
                }
                projected.push(new_row);
            }
            Ok(projected)
        }

        LogicalPlan::CreateNode { labels, properties, alias } => {
            let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            let empty_row = HashMap::new();
            let mut props = PropertyMap::new();
            for (key, expr) in properties {
                let val = eval_expr(expr, &empty_row, &ctx.params)?;
                props.insert(key.clone(), val);
            }
            let node_id = backend.create_node(tx, &label_refs, props).await?;
            ctx.stats.nodes_created += 1;

            let node = backend.get_node(tx, node_id).await?
                .ok_or_else(|| Error::ExecutionError("Created node not found".into()))?;
            let mut row = HashMap::new();
            row.insert(alias.clone(), Value::Node(Box::new(node)));
            if !ctx.columns.contains(alias) {
                ctx.columns.push(alias.clone());
            }
            Ok(vec![row])
        }

        LogicalPlan::CreateRel { src, dst, rel_type, properties } => {
            // src and dst are aliases that must be resolved from a preceding pipeline
            // For standalone CREATE ()-[r:T]->(), we need the node IDs
            // This simplified version expects src/dst to be node IDs encoded in params
            let empty_row = HashMap::new();
            let mut props = PropertyMap::new();
            for (key, expr) in properties {
                let val = eval_expr(expr, &empty_row, &ctx.params)?;
                props.insert(key.clone(), val);
            }
            // For now, src/dst must be numeric params
            let src_id = ctx.params.get(src)
                .and_then(|v| v.as_int())
                .map(|i| NodeId(i as u64))
                .ok_or_else(|| Error::ExecutionError(format!("Cannot resolve source node '{src}'")))?;
            let dst_id = ctx.params.get(dst)
                .and_then(|v| v.as_int())
                .map(|i| NodeId(i as u64))
                .ok_or_else(|| Error::ExecutionError(format!("Cannot resolve target node '{dst}'")))?;

            backend.create_relationship(tx, src_id, dst_id, rel_type, props).await?;
            ctx.stats.relationships_created += 1;
            Ok(vec![HashMap::new()])
        }

        LogicalPlan::Limit { input, count } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            Ok(rows.into_iter().take(*count).collect())
        }

        LogicalPlan::Sort { input, keys } => {
            let mut rows = execute_plan(backend, tx, input, ctx).await?;
            let params = ctx.params.clone();
            // Sort is best-effort — errors during eval are treated as NULL
            rows.sort_by(|a, b| {
                for (expr, ascending) in keys {
                    let va = eval_expr(expr, a, &params).unwrap_or(Value::Null);
                    let vb = eval_expr(expr, b, &params).unwrap_or(Value::Null);
                    if let Some(ord) = va.neo4j_cmp(&vb) {
                        let ord = if *ascending { ord } else { ord.reverse() };
                        if ord != std::cmp::Ordering::Equal {
                            return ord;
                        }
                    }
                }
                std::cmp::Ordering::Equal
            });
            Ok(rows)
        }

        LogicalPlan::CartesianProduct { left, right } => {
            let left_rows = execute_plan(backend, tx, left, ctx).await?;
            let right_rows = execute_plan(backend, tx, right, ctx).await?;
            let mut result = Vec::new();
            for lr in &left_rows {
                for rr in &right_rows {
                    let mut row = lr.clone();
                    row.extend(rr.clone());
                    result.push(row);
                }
            }
            Ok(result)
        }

        LogicalPlan::CallProcedure { name, args, yields } => {
            let empty_row = HashMap::new();
            let arg_vals: Vec<Value> = args.iter()
                .map(|a| eval_expr(a, &empty_row, &ctx.params))
                .collect::<Result<_>>()?;
            let proc_result = backend.call_procedure(tx, name, arg_vals).await?;

            for col in yields {
                if !ctx.columns.contains(col) {
                    ctx.columns.push(col.clone());
                }
            }

            let rows: Vec<Row> = proc_result.rows.into_iter().map(|pr| {
                let mut row = HashMap::new();
                for col in yields {
                    if let Some(val) = pr.get(col) {
                        row.insert(col.clone(), val.clone());
                    }
                }
                row
            }).collect();
            Ok(rows)
        }

        LogicalPlan::Aggregate { input, group_by, aggregations } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            let result = aggregate_rows(&rows, group_by, aggregations, &ctx.params)?;

            ctx.columns.clear();
            for (_, alias) in group_by {
                ctx.columns.push(alias.clone());
            }
            for (_, alias) in aggregations {
                ctx.columns.push(alias.clone());
            }
            Ok(result)
        }

        LogicalPlan::Distinct { input } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            let mut seen = Vec::new();
            let mut result = Vec::new();
            for row in rows {
                // Serialize the row values for dedup — simple but works
                let key: Vec<(String, String)> = row.iter()
                    .map(|(k, v)| (k.clone(), format!("{v}")))
                    .collect();
                if !seen.contains(&key) {
                    seen.push(key);
                    result.push(row);
                }
            }
            Ok(result)
        }

        LogicalPlan::Skip { input, count } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            Ok(rows.into_iter().skip(*count).collect())
        }

        LogicalPlan::SetProperty { input, variable, key, value } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            for row in &rows {
                let val = eval_expr(value, row, &ctx.params)?;
                if let Some(Value::Node(n)) = row.get(variable) {
                    backend.set_node_property(tx, n.id, key, val).await?;
                    ctx.stats.properties_set += 1;
                } else if let Some(Value::Relationship(r)) = row.get(variable) {
                    backend.set_relationship_property(tx, r.id, key, val).await?;
                    ctx.stats.properties_set += 1;
                }
            }
            Ok(rows)
        }

        LogicalPlan::DeleteNode { input, variable, detach } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            for row in &rows {
                if let Some(Value::Node(n)) = row.get(variable) {
                    if *detach {
                        backend.detach_delete_node(tx, n.id).await?;
                    } else {
                        backend.delete_node(tx, n.id).await?;
                    }
                    ctx.stats.nodes_deleted += 1;
                }
            }
            Ok(vec![])
        }

        LogicalPlan::DeleteRel { input, variable } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            for row in &rows {
                if let Some(Value::Relationship(r)) = row.get(variable) {
                    backend.delete_relationship(tx, r.id).await?;
                    ctx.stats.relationships_deleted += 1;
                }
            }
            Ok(vec![])
        }

        LogicalPlan::Unwind { input, expr, alias } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            let mut result = Vec::new();
            for row in &rows {
                let val = eval_expr(expr, row, &ctx.params)?;
                if let Value::List(items) = val {
                    for item in items {
                        let mut new_row = row.clone();
                        new_row.insert(alias.clone(), item);
                        result.push(new_row);
                    }
                } else {
                    // UNWIND on non-list: single row
                    let mut new_row = row.clone();
                    new_row.insert(alias.clone(), val);
                    result.push(new_row);
                }
            }
            if !ctx.columns.contains(alias) {
                ctx.columns.push(alias.clone());
            }
            Ok(result)
        }
        LogicalPlan::RemoveProperty { input, variable, key } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            for row in &rows {
                if let Some(Value::Node(n)) = row.get(variable) {
                    backend.set_node_property(tx, n.id, key, Value::Null).await?;
                    ctx.stats.properties_set += 1;
                } else if let Some(Value::Relationship(r)) = row.get(variable) {
                    backend.set_relationship_property(tx, r.id, key, Value::Null).await?;
                    ctx.stats.properties_set += 1;
                }
            }
            Ok(rows)
        }

        LogicalPlan::RemoveLabel { input, variable, label } => {
            let rows = execute_plan(backend, tx, input, ctx).await?;
            for row in &rows {
                if let Some(Value::Node(n)) = row.get(variable) {
                    backend.remove_label(tx, n.id, label).await?;
                    ctx.stats.labels_removed += 1;
                }
            }
            Ok(rows)
        }
    }
    }) // close Box::pin(async move { ... })
}

// ============================================================================
// Expression evaluator
// ============================================================================

/// Evaluate a Cypher expression against a row of bound variables.
fn eval_expr(expr: &Expr, row: &Row, params: &PropertyMap) -> Result<Value> {
    match expr {
        Expr::Literal(lit) => Ok(match lit {
            Literal::Null => Value::Null,
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Int(i) => Value::Int(*i),
            Literal::Float(f) => Value::Float(*f),
            Literal::String(s) => Value::String(s.clone()),
        }),

        Expr::Variable(name) => {
            row.get(name).cloned().ok_or_else(|| {
                Error::SemanticError(format!("Unbound variable: {name}"))
            })
        }

        Expr::Parameter(name) => {
            params.get(name).cloned().ok_or_else(|| {
                Error::SemanticError(format!("Missing parameter: ${name}"))
            })
        }

        Expr::Property { expr: inner, key } => {
            let val = eval_expr(inner, row, params)?;
            match val {
                Value::Node(n) => Ok(n.get(key).cloned().unwrap_or(Value::Null)),
                Value::Relationship(r) => Ok(r.properties.get(key).cloned().unwrap_or(Value::Null)),
                Value::Map(m) => Ok(m.get(key).cloned().unwrap_or(Value::Null)),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError {
                    expected: "Node, Relationship, or Map".into(),
                    got: val.type_name().into(),
                }),
            }
        }

        Expr::FunctionCall { name, args, distinct: _ } => {
            eval_function(name, args, row, params)
        }

        Expr::BinaryOp { left, op, right } => {
            let lv = eval_expr(left, row, params)?;
            // Short-circuit for AND/OR
            match op {
                BinaryOp::And => {
                    if !lv.is_truthy() { return Ok(Value::Bool(false)); }
                    let rv = eval_expr(right, row, params)?;
                    return Ok(Value::Bool(rv.is_truthy()));
                }
                BinaryOp::Or => {
                    if lv.is_truthy() { return Ok(Value::Bool(true)); }
                    let rv = eval_expr(right, row, params)?;
                    return Ok(Value::Bool(rv.is_truthy()));
                }
                _ => {}
            }
            let rv = eval_expr(right, row, params)?;
            eval_binary_op(&lv, *op, &rv)
        }

        Expr::UnaryOp { op, expr: inner } => {
            let val = eval_expr(inner, row, params)?;
            match op {
                UnaryOp::Not => match val {
                    Value::Null => Ok(Value::Null),
                    Value::Bool(b) => Ok(Value::Bool(!b)),
                    _ => Ok(Value::Bool(!val.is_truthy())),
                },
                UnaryOp::Negate => match val {
                    Value::Int(i) => Ok(Value::Int(-i)),
                    Value::Float(f) => Ok(Value::Float(-f)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(Error::TypeError {
                        expected: "Numeric".into(),
                        got: val.type_name().into(),
                    }),
                },
            }
        }

        Expr::List(items) => {
            let vals: Vec<Value> = items.iter()
                .map(|e| eval_expr(e, row, params))
                .collect::<Result<_>>()?;
            Ok(Value::List(vals))
        }

        Expr::MapLiteral(entries) => {
            let mut map = HashMap::new();
            for (k, v) in entries {
                map.insert(k.clone(), eval_expr(v, row, params)?);
            }
            Ok(Value::Map(map))
        }

        Expr::IsNull { expr: inner, negated } => {
            let val = eval_expr(inner, row, params)?;
            let is_null = val.is_null();
            Ok(Value::Bool(if *negated { !is_null } else { is_null }))
        }

        Expr::In { expr: item, list } => {
            let item_val = eval_expr(item, row, params)?;
            let list_val = eval_expr(list, row, params)?;
            match list_val {
                Value::Null => Ok(Value::Null),
                Value::List(items) => {
                    if item_val.is_null() {
                        Ok(Value::Null)
                    } else {
                        Ok(Value::Bool(items.iter().any(|v| *v == item_val)))
                    }
                }
                _ => Err(Error::TypeError {
                    expected: "List".into(),
                    got: list_val.type_name().into(),
                }),
            }
        }

        Expr::HasLabel { expr: inner, label } => {
            let val = eval_expr(inner, row, params)?;
            match val {
                Value::Node(n) => Ok(Value::Bool(n.has_label(label))),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError {
                    expected: "Node".into(),
                    got: val.type_name().into(),
                }),
            }
        }

        Expr::StringOp { left, op, right } => {
            let lv = eval_expr(left, row, params)?;
            let rv = eval_expr(right, row, params)?;
            match (&lv, &rv) {
                (Value::String(a), Value::String(b)) => {
                    let result = match op {
                        StringOp::StartsWith => a.starts_with(b.as_str()),
                        StringOp::EndsWith => a.ends_with(b.as_str()),
                        StringOp::Contains => a.contains(b.as_str()),
                    };
                    Ok(Value::Bool(result))
                }
                (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
                _ => Err(Error::TypeError {
                    expected: "String".into(),
                    got: format!("{}, {}", lv.type_name(), rv.type_name()),
                }),
            }
        }

        Expr::Star => {
            // In RETURN *, return all bound variables
            // This is handled at the Project level
            Ok(Value::Null)
        }

        Expr::Case { operand, whens, else_expr } => {
            if let Some(op) = operand {
                // Simple CASE: CASE x WHEN val THEN result
                let op_val = eval_expr(op, row, params)?;
                for (when_expr, then_expr) in whens {
                    let when_val = eval_expr(when_expr, row, params)?;
                    if op_val == when_val {
                        return eval_expr(then_expr, row, params);
                    }
                }
            } else {
                // Searched CASE: CASE WHEN cond THEN result
                for (when_expr, then_expr) in whens {
                    let when_val = eval_expr(when_expr, row, params)?;
                    if when_val.is_truthy() {
                        return eval_expr(then_expr, row, params);
                    }
                }
            }
            if let Some(else_e) = else_expr {
                eval_expr(else_e, row, params)
            } else {
                Ok(Value::Null)
            }
        }

        Expr::Exists(_) => {
            // EXISTS subqueries need the full backend — simplify for now
            Err(Error::ExecutionError("EXISTS subquery not yet supported in execution".into()))
        }
    }
}

// ============================================================================
// Binary operator evaluation
// ============================================================================

fn eval_binary_op(left: &Value, op: BinaryOp, right: &Value) -> Result<Value> {
    // NULL propagation for most operators
    if left.is_null() || right.is_null() {
        return match op {
            BinaryOp::Eq | BinaryOp::Neq => Ok(Value::Null),
            _ => Ok(Value::Null),
        };
    }

    match op {
        // Comparison
        BinaryOp::Eq => Ok(Value::Bool(left == right)),
        BinaryOp::Neq => Ok(Value::Bool(left != right)),
        BinaryOp::Lt => Ok(Value::Bool(left.neo4j_cmp(right) == Some(std::cmp::Ordering::Less))),
        BinaryOp::Lte => Ok(Value::Bool(matches!(left.neo4j_cmp(right), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)))),
        BinaryOp::Gt => Ok(Value::Bool(left.neo4j_cmp(right) == Some(std::cmp::Ordering::Greater))),
        BinaryOp::Gte => Ok(Value::Bool(matches!(left.neo4j_cmp(right), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)))),

        // Arithmetic
        BinaryOp::Add => eval_add(left, right),
        BinaryOp::Sub => eval_arith(left, right, |a, b| a - b, |a, b| a - b),
        BinaryOp::Mul => eval_arith(left, right, |a, b| a * b, |a, b| a * b),
        BinaryOp::Div => {
            // Division by zero check
            match right {
                Value::Int(0) => Err(Error::ExecutionError("Division by zero".into())),
                Value::Float(f) if *f == 0.0 => Err(Error::ExecutionError("Division by zero".into())),
                _ => eval_arith(left, right, |a, b| a / b, |a, b| a / b),
            }
        }
        BinaryOp::Mod => eval_arith(left, right, |a, b| a % b, |a, b| a % b),
        BinaryOp::Pow => {
            let l = left.as_float().ok_or_else(|| Error::TypeError {
                expected: "Numeric".into(), got: left.type_name().into(),
            })?;
            let r = right.as_float().ok_or_else(|| Error::TypeError {
                expected: "Numeric".into(), got: right.type_name().into(),
            })?;
            Ok(Value::Float(l.powf(r)))
        }

        // Logical (non-short-circuit path, NULLs already handled)
        BinaryOp::And => Ok(Value::Bool(left.is_truthy() && right.is_truthy())),
        BinaryOp::Or => Ok(Value::Bool(left.is_truthy() || right.is_truthy())),
        BinaryOp::Xor => Ok(Value::Bool(left.is_truthy() ^ right.is_truthy())),

        // Regex
        BinaryOp::RegexMatch => {
            match (left, right) {
                (Value::String(_s), Value::String(_pattern)) => {
                    // Would need regex crate — return error for now
                    Err(Error::ExecutionError("Regex not yet supported".into()))
                }
                _ => Err(Error::TypeError {
                    expected: "String".into(),
                    got: format!("{}, {}", left.type_name(), right.type_name()),
                }),
            }
        }
    }
}

fn eval_add(left: &Value, right: &Value) -> Result<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
        (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
        (Value::List(a), Value::List(b)) => {
            let mut result = a.clone();
            result.extend(b.clone());
            Ok(Value::List(result))
        }
        _ => Err(Error::TypeError {
            expected: "compatible types for +".into(),
            got: format!("{}, {}", left.type_name(), right.type_name()),
        }),
    }
}

fn eval_arith(
    left: &Value,
    right: &Value,
    int_op: fn(i64, i64) -> i64,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(*a, *b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(*a, *b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(*a as f64, *b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(*a, *b as f64))),
        _ => Err(Error::TypeError {
            expected: "Numeric".into(),
            got: format!("{}, {}", left.type_name(), right.type_name()),
        }),
    }
}

// ============================================================================
// Built-in function evaluation
// ============================================================================

fn eval_function(name: &str, args: &[Expr], row: &Row, params: &PropertyMap) -> Result<Value> {
    let upper = name.to_uppercase();
    match upper.as_str() {
        "ID" => {
            let val = eval_expr(args.first().ok_or_else(|| Error::ExecutionError("id() requires 1 argument".into()))?, row, params)?;
            match val {
                Value::Node(n) => Ok(Value::Int(n.id.0 as i64)),
                Value::Relationship(r) => Ok(Value::Int(r.id.0 as i64)),
                _ => Err(Error::TypeError { expected: "Node or Relationship".into(), got: val.type_name().into() }),
            }
        }
        "LABELS" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Node(n) => Ok(Value::List(n.labels.iter().map(|l| Value::String(l.clone())).collect())),
                _ => Err(Error::TypeError { expected: "Node".into(), got: val.type_name().into() }),
            }
        }
        "TYPE" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Relationship(r) => Ok(Value::String(r.rel_type.clone())),
                _ => Err(Error::TypeError { expected: "Relationship".into(), got: val.type_name().into() }),
            }
        }
        "PROPERTIES" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Node(n) => Ok(Value::Map(n.properties.clone())),
                Value::Relationship(r) => Ok(Value::Map(r.properties.clone())),
                _ => Err(Error::TypeError { expected: "Node or Relationship".into(), got: val.type_name().into() }),
            }
        }
        "KEYS" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Node(n) => Ok(Value::List(n.properties.keys().map(|k| Value::String(k.clone())).collect())),
                Value::Relationship(r) => Ok(Value::List(r.properties.keys().map(|k| Value::String(k.clone())).collect())),
                Value::Map(m) => Ok(Value::List(m.keys().map(|k| Value::String(k.clone())).collect())),
                _ => Err(Error::TypeError { expected: "Node, Relationship, or Map".into(), got: val.type_name().into() }),
            }
        }
        "TOINTEGER" | "TOINT" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Int(_) => Ok(val),
                Value::Float(f) => Ok(Value::Int(f as i64)),
                Value::String(s) => s.parse::<i64>().map(Value::Int).map_err(|_| Error::TypeError { expected: "parseable integer".into(), got: s }),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "convertible to Integer".into(), got: val.type_name().into() }),
            }
        }
        "TOFLOAT" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Float(_) => Ok(val),
                Value::Int(i) => Ok(Value::Float(i as f64)),
                Value::String(s) => s.parse::<f64>().map(Value::Float).map_err(|_| Error::TypeError { expected: "parseable float".into(), got: s }),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "convertible to Float".into(), got: val.type_name().into() }),
            }
        }
        "TOSTRING" => {
            let val = eval_expr(&args[0], row, params)?;
            Ok(Value::String(format!("{val}")))
        }
        "TOBOOLEAN" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Bool(_) => Ok(val),
                Value::String(s) => match s.to_lowercase().as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => Ok(Value::Null),
                },
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "convertible to Boolean".into(), got: val.type_name().into() }),
            }
        }
        "SIZE" | "LENGTH" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::String(s) => Ok(Value::Int(s.len() as i64)),
                Value::List(l) => Ok(Value::Int(l.len() as i64)),
                Value::Path(p) => Ok(Value::Int(p.len() as i64)),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "String, List, or Path".into(), got: val.type_name().into() }),
            }
        }
        "HEAD" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::List(l) => Ok(l.into_iter().next().unwrap_or(Value::Null)),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "List".into(), got: val.type_name().into() }),
            }
        }
        "LAST" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::List(l) => Ok(l.into_iter().last().unwrap_or(Value::Null)),
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "List".into(), got: val.type_name().into() }),
            }
        }
        "TAIL" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::List(mut l) => { if !l.is_empty() { l.remove(0); } Ok(Value::List(l)) }
                Value::Null => Ok(Value::Null),
                _ => Err(Error::TypeError { expected: "List".into(), got: val.type_name().into() }),
            }
        }
        "RANGE" => {
            let start = eval_expr(&args[0], row, params)?.as_int()
                .ok_or_else(|| Error::TypeError { expected: "Integer".into(), got: "non-integer".into() })?;
            let end = eval_expr(&args[1], row, params)?.as_int()
                .ok_or_else(|| Error::TypeError { expected: "Integer".into(), got: "non-integer".into() })?;
            let step = if args.len() > 2 {
                eval_expr(&args[2], row, params)?.as_int()
                    .ok_or_else(|| Error::TypeError { expected: "Integer".into(), got: "non-integer".into() })?
            } else { 1 };
            let mut list = Vec::new();
            let mut i = start;
            while (step > 0 && i <= end) || (step < 0 && i >= end) {
                list.push(Value::Int(i));
                i += step;
            }
            Ok(Value::List(list))
        }
        "COALESCE" => {
            for arg in args {
                let val = eval_expr(arg, row, params)?;
                if !val.is_null() {
                    return Ok(val);
                }
            }
            Ok(Value::Null)
        }
        "NODES" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Path(p) => Ok(Value::List(p.nodes.into_iter().map(|n| Value::Node(Box::new(n))).collect())),
                _ => Err(Error::TypeError { expected: "Path".into(), got: val.type_name().into() }),
            }
        }
        "RELATIONSHIPS" | "RELS" => {
            let val = eval_expr(&args[0], row, params)?;
            match val {
                Value::Path(p) => Ok(Value::List(p.relationships.into_iter().map(|r| Value::Relationship(Box::new(r))).collect())),
                _ => Err(Error::TypeError { expected: "Path".into(), got: val.type_name().into() }),
            }
        }
        // Aggregation functions are placeholders — real aggregation is done in aggregate_rows
        "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "COLLECT" => {
            // When called per-row (not in aggregation context), just evaluate the arg
            if args.is_empty() {
                Ok(Value::Int(1)) // count(*)
            } else {
                eval_expr(&args[0], row, params)
            }
        }
        _ => Err(Error::ExecutionError(format!("Unknown function: {name}"))),
    }
}

// ============================================================================
// Aggregation
// ============================================================================

fn aggregate_rows(
    rows: &[Row],
    group_by: &[(Expr, String)],
    aggregations: &[(Expr, String)],
    params: &PropertyMap,
) -> Result<Vec<Row>> {
    // Group rows by group-by key values
    let mut groups: Vec<(Vec<Value>, Vec<&Row>)> = Vec::new();

    for row in rows {
        let key: Vec<Value> = group_by.iter()
            .map(|(expr, _)| eval_expr(expr, row, params).unwrap_or(Value::Null))
            .collect();

        if let Some(group) = groups.iter_mut().find(|(k, _)| *k == key) {
            group.1.push(row);
        } else {
            groups.push((key, vec![row]));
        }
    }

    // If no group_by and no rows, produce one row with default aggregation values
    if groups.is_empty() && group_by.is_empty() {
        let mut result_row = HashMap::new();
        for (expr, alias) in aggregations {
            let val = compute_aggregate(expr, &[], params)?;
            result_row.insert(alias.clone(), val);
        }
        return Ok(vec![result_row]);
    }

    let mut result = Vec::new();
    for (key_vals, group_rows) in &groups {
        let mut row = HashMap::new();
        // Insert group-by values
        for (i, (_, alias)) in group_by.iter().enumerate() {
            row.insert(alias.clone(), key_vals[i].clone());
        }
        // Compute aggregations
        for (expr, alias) in aggregations {
            let val = compute_aggregate(expr, group_rows, params)?;
            row.insert(alias.clone(), val);
        }
        result.push(row);
    }
    Ok(result)
}

fn compute_aggregate(expr: &Expr, rows: &[&Row], params: &PropertyMap) -> Result<Value> {
    match expr {
        Expr::FunctionCall { name, args, distinct } => {
            let upper = name.to_uppercase();
            let vals: Vec<Value> = if args.is_empty() {
                // count(*) — count all rows
                vec![]
            } else {
                let mut v = Vec::new();
                for row in rows {
                    let val = eval_expr(&args[0], row, params)?;
                    if !val.is_null() {
                        v.push(val);
                    }
                }
                if *distinct {
                    let mut deduped = Vec::new();
                    for val in v {
                        if !deduped.contains(&val) {
                            deduped.push(val);
                        }
                    }
                    deduped
                } else {
                    v
                }
            };

            match upper.as_str() {
                "COUNT" => {
                    if args.is_empty() {
                        Ok(Value::Int(rows.len() as i64))
                    } else {
                        Ok(Value::Int(vals.len() as i64))
                    }
                }
                "SUM" => {
                    let mut sum_i: i64 = 0;
                    let mut sum_f: f64 = 0.0;
                    let mut has_float = false;
                    for val in &vals {
                        match val {
                            Value::Int(i) => sum_i += i,
                            Value::Float(f) => { has_float = true; sum_f += f; }
                            _ => {}
                        }
                    }
                    if has_float {
                        Ok(Value::Float(sum_i as f64 + sum_f))
                    } else {
                        Ok(Value::Int(sum_i))
                    }
                }
                "AVG" => {
                    if vals.is_empty() { return Ok(Value::Null); }
                    let mut sum: f64 = 0.0;
                    for val in &vals {
                        sum += val.as_float().unwrap_or(0.0);
                    }
                    Ok(Value::Float(sum / vals.len() as f64))
                }
                "MIN" => {
                    vals.into_iter().reduce(|a, b| {
                        if a.neo4j_cmp(&b) == Some(std::cmp::Ordering::Less) { a } else { b }
                    }).map(Ok).unwrap_or(Ok(Value::Null))
                }
                "MAX" => {
                    vals.into_iter().reduce(|a, b| {
                        if a.neo4j_cmp(&b) == Some(std::cmp::Ordering::Greater) { a } else { b }
                    }).map(Ok).unwrap_or(Ok(Value::Null))
                }
                "COLLECT" => {
                    Ok(Value::List(vals))
                }
                _ => Err(Error::ExecutionError(format!("Unknown aggregate: {name}"))),
            }
        }
        // Non-aggregate expressions in aggregation context — just eval against first row
        other => {
            if let Some(row) = rows.first() {
                eval_expr(other, row, params)
            } else {
                Ok(Value::Null)
            }
        }
    }
}
