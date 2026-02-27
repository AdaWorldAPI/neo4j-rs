//! Query planner — transforms Cypher AST into logical/physical plans.
//!
//! The planner is backend-agnostic. It produces logical operators that
//! the execution engine maps to StorageBackend calls.

use crate::model::PropertyMap;
use crate::cypher::ast::{self, *};
use crate::{Error, Result};

/// Logical plan node.
#[derive(Debug, Clone)]
pub enum LogicalPlan {
    /// Scan all nodes with a given label
    NodeScan { label: String, alias: String },
    /// Scan ALL nodes (no label filter)
    AllNodesScan { alias: String },
    /// Index-backed property lookup
    IndexLookup { label: String, property: String, alias: String },
    /// Expand relationships from a node (piped from input plan)
    Expand { input: Box<LogicalPlan>, from: String, dir: crate::model::Direction, rel_types: Vec<String>, to: String, rel_alias: Option<String> },
    /// Filter rows by predicate
    Filter { input: Box<LogicalPlan>, predicate: Expr },
    /// Project columns
    Project { input: Box<LogicalPlan>, items: Vec<(Expr, String)> },
    /// Create node
    CreateNode { labels: Vec<String>, properties: Vec<(String, Expr)>, alias: String },
    /// Create relationship
    CreateRel { src: String, dst: String, rel_type: String, properties: Vec<(String, Expr)> },
    /// Limit output rows
    Limit { input: Box<LogicalPlan>, count: usize },
    /// Skip first N rows
    Skip { input: Box<LogicalPlan>, count: usize },
    /// Sort
    Sort { input: Box<LogicalPlan>, keys: Vec<(Expr, bool)> },
    /// Cartesian product of two inputs
    CartesianProduct { left: Box<LogicalPlan>, right: Box<LogicalPlan> },
    /// Call a procedure: CALL name(args) YIELD columns
    CallProcedure { name: String, args: Vec<Expr>, yields: Vec<String> },
    /// Empty leaf (produces one empty row)
    Argument,
    /// Aggregate: group-by keys + aggregation expressions
    Aggregate { input: Box<LogicalPlan>, group_by: Vec<(Expr, String)>, aggregations: Vec<(Expr, String)> },
    /// Distinct (dedup rows)
    Distinct { input: Box<LogicalPlan> },
    /// SET n.key = expr
    SetProperty { input: Box<LogicalPlan>, variable: String, key: String, value: Expr },
    /// DELETE n (or DETACH DELETE n)
    DeleteNode { input: Box<LogicalPlan>, variable: String, detach: bool },
    /// DELETE r
    DeleteRel { input: Box<LogicalPlan>, variable: String },
    /// UNWIND list AS x
    Unwind { input: Box<LogicalPlan>, expr: Expr, alias: String },
    /// REMOVE n.key (set property to NULL)
    RemoveProperty { input: Box<LogicalPlan>, variable: String, key: String },
    /// REMOVE n:Label
    RemoveLabel { input: Box<LogicalPlan>, variable: String, label: String },
    /// MERGE (upsert): match-or-create a node/pattern
    MergeNode {
        labels: Vec<String>,
        properties: Vec<(String, Expr)>,
        alias: String,
        on_create: Vec<(String, String, Expr)>,
        on_match: Vec<(String, String, Expr)>,
    },
    /// CREATE INDEX / CREATE CONSTRAINT / DROP INDEX / DROP CONSTRAINT
    SchemaOp(SchemaCommand),
}

/// Create a logical plan from a parsed AST.
pub fn plan(ast: &Statement, params: &PropertyMap) -> Result<LogicalPlan> {
    let _ = params; // used by optimize() later
    match ast {
        Statement::Query(q) => plan_query(q),
        Statement::Create(c) => plan_create(c),
        Statement::Delete(d) => plan_delete(d),
        Statement::Set(s) => plan_set(s),
        Statement::Merge(m) => plan_merge(m),
        Statement::Schema(s) => Ok(LogicalPlan::SchemaOp(s.clone())),
        Statement::Remove(r) => plan_remove(r),
    }
}

fn plan_query(q: &Query) -> Result<LogicalPlan> {
    let mut current = if q.matches.is_empty() {
        LogicalPlan::Argument
    } else {
        plan_matches(&q.matches)?
    };

    if let Some(ref where_expr) = q.where_clause {
        current = LogicalPlan::Filter {
            input: Box::new(current),
            predicate: where_expr.clone(),
        };
    }

    // Sort BEFORE projection so ORDER BY expressions can reference
    // pre-projection variables (e.g. n.name, n.age). Neo4j semantics.
    if let Some(ref order) = q.order_by {
        let keys: Vec<(Expr, bool)> = order.iter()
            .map(|o| (o.expr.clone(), o.ascending))
            .collect();
        current = LogicalPlan::Sort { input: Box::new(current), keys };
    }

    let (has_agg, group_by, aggregations, _plain) = classify_return_items(&q.return_clause);

    if has_agg {
        current = LogicalPlan::Aggregate {
            input: Box::new(current),
            group_by,
            aggregations,
        };
    } else {
        let items: Vec<(Expr, String)> = q.return_clause.items.iter().map(|item| {
            let alias = item.alias.clone().unwrap_or_else(|| expr_default_alias(&item.expr));
            (item.expr.clone(), alias)
        }).collect();
        current = LogicalPlan::Project {
            input: Box::new(current),
            items,
        };
    }

    if q.return_clause.distinct {
        current = LogicalPlan::Distinct { input: Box::new(current) };
    }

    if let Some(ref skip_expr) = q.skip {
        if let Expr::Literal(Literal::Int(n)) = skip_expr {
            current = LogicalPlan::Skip { input: Box::new(current), count: *n as usize };
        }
    }

    if let Some(ref limit_expr) = q.limit {
        if let Expr::Literal(Literal::Int(n)) = limit_expr {
            current = LogicalPlan::Limit { input: Box::new(current), count: *n as usize };
        }
    }

    Ok(current)
}

fn plan_matches(matches: &[MatchClause]) -> Result<LogicalPlan> {
    let mut plans = Vec::new();
    for m in matches {
        for pattern in &m.patterns {
            plans.push(plan_pattern(pattern)?);
        }
    }

    if plans.is_empty() {
        return Ok(LogicalPlan::Argument);
    }

    let mut current = plans.remove(0);
    for p in plans {
        current = LogicalPlan::CartesianProduct {
            left: Box::new(current),
            right: Box::new(p),
        };
    }
    Ok(current)
}

fn plan_pattern(pattern: &Pattern) -> Result<LogicalPlan> {
    if pattern.elements.is_empty() {
        return Ok(LogicalPlan::Argument);
    }

    let mut plan: Option<LogicalPlan> = None;
    let mut last_alias: Option<String> = None;
    let mut i = 0;

    while i < pattern.elements.len() {
        match &pattern.elements[i] {
            PatternElement::Node(np) => {
                let alias = np.alias.clone().unwrap_or_else(|| format!("_anon_{}", next_id()));
                if plan.is_none() {
                    plan = Some(if np.labels.is_empty() {
                        LogicalPlan::AllNodesScan { alias: alias.clone() }
                    } else {
                        LogicalPlan::NodeScan {
                            label: np.labels[0].clone(),
                            alias: alias.clone(),
                        }
                    });
                }
                last_alias = Some(alias);
                i += 1;
            }
            PatternElement::Relationship(rp) => {
                let from = last_alias.clone().ok_or_else(|| {
                    Error::PlanError("Relationship pattern without preceding node".into())
                })?;

                i += 1;
                let to_alias = if i < pattern.elements.len() {
                    if let PatternElement::Node(to_np) = &pattern.elements[i] {
                        let a = to_np.alias.clone().unwrap_or_else(|| format!("_anon_{}", next_id()));
                        i += 1;
                        a
                    } else {
                        return Err(Error::PlanError("Expected node after relationship".into()));
                    }
                } else {
                    return Err(Error::PlanError("Relationship pattern must end with node".into()));
                };

                let dir = match rp.direction {
                    PatternDirection::Right => crate::model::Direction::Outgoing,
                    PatternDirection::Left => crate::model::Direction::Incoming,
                    PatternDirection::Both => crate::model::Direction::Both,
                };

                let input = plan.take().unwrap_or(LogicalPlan::Argument);
                plan = Some(LogicalPlan::Expand {
                    input: Box::new(input),
                    from,
                    dir,
                    rel_types: rp.rel_types.clone(),
                    to: to_alias.clone(),
                    rel_alias: rp.alias.clone(),
                });
                last_alias = Some(to_alias);
            }
        }
    }

    plan.ok_or_else(|| Error::PlanError("Empty pattern".into()))
}

fn plan_create(c: &CreateClause) -> Result<LogicalPlan> {
    let mut plans = Vec::new();

    for pattern in &c.patterns {
        for elem in &pattern.elements {
            if let PatternElement::Node(np) = elem {
                let alias = np.alias.clone().unwrap_or_else(|| format!("_anon_{}", next_id()));
                let properties: Vec<(String, Expr)> = np.properties.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                plans.push(LogicalPlan::CreateNode {
                    labels: np.labels.clone(),
                    properties,
                    alias,
                });
            }
        }
    }

    if plans.is_empty() {
        return Ok(LogicalPlan::Argument);
    }

    let mut current = plans.remove(0);
    for p in plans {
        current = LogicalPlan::CartesianProduct {
            left: Box::new(current),
            right: Box::new(p),
        };
    }

    if let Some(ref ret) = c.return_clause {
        let items: Vec<(Expr, String)> = ret.items.iter().map(|item| {
            let alias = item.alias.clone().unwrap_or_else(|| expr_default_alias(&item.expr));
            (item.expr.clone(), alias)
        }).collect();
        current = LogicalPlan::Project {
            input: Box::new(current),
            items,
        };
    }

    Ok(current)
}

fn plan_delete(d: &DeleteClause) -> Result<LogicalPlan> {
    let mut current = if d.matches.is_empty() {
        LogicalPlan::Argument
    } else {
        plan_matches(&d.matches)?
    };

    if let Some(ref where_expr) = d.where_clause {
        current = LogicalPlan::Filter {
            input: Box::new(current),
            predicate: where_expr.clone(),
        };
    }

    for var in &d.variables {
        current = LogicalPlan::DeleteNode {
            input: Box::new(current),
            variable: var.clone(),
            detach: d.detach,
        };
    }

    Ok(current)
}

fn plan_remove(r: &RemoveClause) -> Result<LogicalPlan> {
    let mut current = if r.matches.is_empty() {
        LogicalPlan::Argument
    } else {
        plan_matches(&r.matches)?
    };

    if let Some(ref where_expr) = r.where_clause {
        current = LogicalPlan::Filter {
            input: Box::new(current),
            predicate: where_expr.clone(),
        };
    }

    for item in &r.items {
        match item {
            RemoveItem::Property { variable, key } => {
                current = LogicalPlan::RemoveProperty {
                    input: Box::new(current),
                    variable: variable.clone(),
                    key: key.clone(),
                };
            }
            RemoveItem::Label { variable, label } => {
                current = LogicalPlan::RemoveLabel {
                    input: Box::new(current),
                    variable: variable.clone(),
                    label: label.clone(),
                };
            }
        }
    }

    if let Some(ref ret) = r.return_clause {
        let items: Vec<(Expr, String)> = ret.items.iter().map(|item| {
            let alias = item.alias.clone().unwrap_or_else(|| expr_default_alias(&item.expr));
            (item.expr.clone(), alias)
        }).collect();
        current = LogicalPlan::Project {
            input: Box::new(current),
            items,
        };
    }

    Ok(current)
}

fn plan_set(s: &SetClause) -> Result<LogicalPlan> {
    let mut current = if s.matches.is_empty() {
        LogicalPlan::Argument
    } else {
        plan_matches(&s.matches)?
    };

    if let Some(ref where_expr) = s.where_clause {
        current = LogicalPlan::Filter {
            input: Box::new(current),
            predicate: where_expr.clone(),
        };
    }

    for item in &s.items {
        match item {
            SetItem::Property { variable, key, value } => {
                current = LogicalPlan::SetProperty {
                    input: Box::new(current),
                    variable: variable.clone(),
                    key: key.clone(),
                    value: value.clone(),
                };
            }
            _ => return Err(Error::PlanError("Only SET n.prop = expr is currently supported".into())),
        }
    }

    if let Some(ref ret) = s.return_clause {
        let items: Vec<(Expr, String)> = ret.items.iter().map(|item| {
            let alias = item.alias.clone().unwrap_or_else(|| expr_default_alias(&item.expr));
            (item.expr.clone(), alias)
        }).collect();
        current = LogicalPlan::Project {
            input: Box::new(current),
            items,
        };
    }

    Ok(current)
}

fn plan_merge(m: &MergeClause) -> Result<LogicalPlan> {
    // Extract the node from the MERGE pattern
    let node_pattern = m.pattern.elements.iter().find_map(|e| {
        if let PatternElement::Node(np) = e { Some(np) } else { None }
    }).ok_or_else(|| Error::PlanError("MERGE requires at least one node pattern".into()))?;

    let alias = node_pattern.alias.clone().unwrap_or_else(|| format!("_anon_{}", next_id()));
    let properties: Vec<(String, Expr)> = node_pattern.properties.iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let on_create: Vec<(String, String, Expr)> = m.on_create.iter().filter_map(|item| {
        if let ast::SetItem::Property { variable, key, value } = item {
            Some((variable.clone(), key.clone(), value.clone()))
        } else {
            None
        }
    }).collect();

    let on_match: Vec<(String, String, Expr)> = m.on_match.iter().filter_map(|item| {
        if let ast::SetItem::Property { variable, key, value } = item {
            Some((variable.clone(), key.clone(), value.clone()))
        } else {
            None
        }
    }).collect();

    let mut current = LogicalPlan::MergeNode {
        labels: node_pattern.labels.clone(),
        properties,
        alias: alias.clone(),
        on_create,
        on_match,
    };

    if let Some(ref ret) = m.return_clause {
        let items: Vec<(Expr, String)> = ret.items.iter().map(|item| {
            let alias = item.alias.clone().unwrap_or_else(|| expr_default_alias(&item.expr));
            (item.expr.clone(), alias)
        }).collect();
        current = LogicalPlan::Project {
            input: Box::new(current),
            items,
        };
    }

    Ok(current)
}

// ============================================================================
// Helpers
// ============================================================================

fn classify_return_items(ret: &ReturnClause) -> (bool, Vec<(Expr, String)>, Vec<(Expr, String)>, Vec<(Expr, String)>) {
    let mut has_agg = false;
    let mut group_by = Vec::new();
    let mut aggregations = Vec::new();
    let mut plain = Vec::new();

    for item in &ret.items {
        let alias = item.alias.clone().unwrap_or_else(|| expr_default_alias(&item.expr));
        if is_aggregate_expr(&item.expr) {
            has_agg = true;
            aggregations.push((item.expr.clone(), alias));
        } else {
            plain.push((item.expr.clone(), alias.clone()));
            group_by.push((item.expr.clone(), alias));
        }
    }

    (has_agg, group_by, aggregations, plain)
}

fn is_aggregate_expr(expr: &Expr) -> bool {
    match expr {
        Expr::FunctionCall { name, .. } => {
            matches!(name.to_uppercase().as_str(), "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" | "COLLECT")
        }
        _ => false,
    }
}

fn expr_default_alias(expr: &Expr) -> String {
    match expr {
        Expr::Variable(name) => name.clone(),
        Expr::Property { expr, key } => format!("{}.{}", expr_default_alias(expr), key),
        Expr::FunctionCall { name, .. } => name.clone(),
        Expr::Star => "*".to_string(),
        _ => "_expr".to_string(),
    }
}

fn next_id() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Optimize a logical plan.
pub fn optimize(plan: LogicalPlan) -> Result<LogicalPlan> {
    // TODO: Cost-based optimizer
    // Rules: predicate pushdown, index selection, join ordering
    Ok(plan)
}
