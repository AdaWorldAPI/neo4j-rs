//! Cypher AST (Abstract Syntax Tree)
//!
//! These types represent parsed Cypher queries. They are pure data —
//! no behavior, no storage references, no execution logic.

use std::collections::HashMap;

/// A complete Cypher statement.
#[derive(Debug, Clone)]
pub enum Statement {
    /// Read-only query: MATCH ... RETURN ...
    Query(Query),
    /// Create nodes/relationships: CREATE ...
    Create(CreateClause),
    /// Merge (upsert): MERGE ...
    Merge(MergeClause),
    /// Delete: MATCH ... DELETE ...
    Delete(DeleteClause),
    /// Set properties: MATCH ... SET ...
    Set(SetClause),
    /// Schema commands
    Schema(SchemaCommand),
}

/// A read query (MATCH + RETURN).
#[derive(Debug, Clone)]
pub struct Query {
    pub matches: Vec<MatchClause>,
    pub where_clause: Option<Expr>,
    pub with_clauses: Vec<WithClause>,
    pub return_clause: ReturnClause,
    pub order_by: Option<Vec<OrderExpr>>,
    pub skip: Option<Expr>,
    pub limit: Option<Expr>,
}

/// MATCH clause with pattern and optional WHERE.
#[derive(Debug, Clone)]
pub struct MatchClause {
    pub optional: bool,
    pub patterns: Vec<Pattern>,
}

/// A pattern: (a:Person)-[:KNOWS]->(b:Person)
#[derive(Debug, Clone)]
pub struct Pattern {
    pub elements: Vec<PatternElement>,
}

/// Element of a pattern — either a node or a relationship.
#[derive(Debug, Clone)]
pub enum PatternElement {
    Node(NodePattern),
    Relationship(RelPattern),
}

/// Node pattern: (alias:Label1:Label2 {prop: value})
#[derive(Debug, Clone)]
pub struct NodePattern {
    pub alias: Option<String>,
    pub labels: Vec<String>,
    pub properties: HashMap<String, Expr>,
}

/// Relationship pattern: -[alias:TYPE *min..max {props}]->
#[derive(Debug, Clone)]
pub struct RelPattern {
    pub alias: Option<String>,
    pub rel_types: Vec<String>,
    pub direction: PatternDirection,
    pub properties: HashMap<String, Expr>,
    pub var_length: Option<VarLength>,
}

/// Pattern direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternDirection {
    /// ->
    Right,
    /// <-
    Left,
    /// - (undirected)
    Both,
}

/// Variable-length path specification.
#[derive(Debug, Clone)]
pub struct VarLength {
    pub min: Option<usize>,
    pub max: Option<usize>,
}

/// RETURN clause.
#[derive(Debug, Clone)]
pub struct ReturnClause {
    pub distinct: bool,
    pub items: Vec<ReturnItem>,
}

/// Single item in RETURN.
#[derive(Debug, Clone)]
pub struct ReturnItem {
    pub expr: Expr,
    pub alias: Option<String>,
}

/// WITH clause (pipeline / sub-query boundary).
#[derive(Debug, Clone)]
pub struct WithClause {
    pub items: Vec<ReturnItem>,
    pub where_clause: Option<Expr>,
}

/// ORDER BY expression.
#[derive(Debug, Clone)]
pub struct OrderExpr {
    pub expr: Expr,
    pub ascending: bool,
}

// ============================================================================
// Expressions
// ============================================================================

/// Expression in Cypher.
#[derive(Debug, Clone)]
pub enum Expr {
    /// Literal value
    Literal(Literal),
    /// Variable reference: `n`, `r`, `p`
    Variable(String),
    /// Property access: `n.name`
    Property { expr: Box<Expr>, key: String },
    /// Parameter: `$name`
    Parameter(String),
    /// Function call: `count(n)`, `id(n)`, `labels(n)`
    FunctionCall { name: String, args: Vec<Expr>, distinct: bool },
    /// Binary operation: `a + b`, `a = b`, `a AND b`
    BinaryOp { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    /// Unary operation: `NOT a`, `-a`
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    /// List: `[1, 2, 3]`
    List(Vec<Expr>),
    /// Map: `{name: 'Ada', age: 3}`
    MapLiteral(HashMap<String, Expr>),
    /// CASE expression
    Case { operand: Option<Box<Expr>>, whens: Vec<(Expr, Expr)>, else_expr: Option<Box<Expr>> },
    /// EXISTS subquery
    Exists(Box<MatchClause>),
    /// IN predicate: `x IN [1, 2, 3]`
    In { expr: Box<Expr>, list: Box<Expr> },
    /// IS NULL / IS NOT NULL
    IsNull { expr: Box<Expr>, negated: bool },
    /// Label check: `n:Person`
    HasLabel { expr: Box<Expr>, label: String },
    /// String operations: STARTS WITH, ENDS WITH, CONTAINS
    StringOp { left: Box<Expr>, op: StringOp, right: Box<Expr> },
    /// Wildcard: `*` (in RETURN *)
    Star,
}

/// Literal values.
#[derive(Debug, Clone)]
pub enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    // Arithmetic
    Add, Sub, Mul, Div, Mod, Pow,
    // Comparison
    Eq, Neq, Lt, Lte, Gt, Gte,
    // Logical
    And, Or, Xor,
    // String
    RegexMatch,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Negate,
}

/// String-specific operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringOp {
    StartsWith,
    EndsWith,
    Contains,
}

// ============================================================================
// Write clauses
// ============================================================================

/// CREATE clause.
#[derive(Debug, Clone)]
pub struct CreateClause {
    pub patterns: Vec<Pattern>,
    pub return_clause: Option<ReturnClause>,
}

/// MERGE clause.
#[derive(Debug, Clone)]
pub struct MergeClause {
    pub pattern: Pattern,
    pub on_create: Vec<SetItem>,
    pub on_match: Vec<SetItem>,
    pub return_clause: Option<ReturnClause>,
}

/// DELETE clause.
#[derive(Debug, Clone)]
pub struct DeleteClause {
    pub matches: Vec<MatchClause>,
    pub where_clause: Option<Expr>,
    pub variables: Vec<String>,
    pub detach: bool,
}

/// SET clause.
#[derive(Debug, Clone)]
pub struct SetClause {
    pub matches: Vec<MatchClause>,
    pub where_clause: Option<Expr>,
    pub items: Vec<SetItem>,
    pub return_clause: Option<ReturnClause>,
}

/// Single SET item.
#[derive(Debug, Clone)]
pub enum SetItem {
    /// SET n.prop = expr
    Property { variable: String, key: String, value: Expr },
    /// SET n = {map}
    AllProperties { variable: String, value: Expr },
    /// SET n += {map}
    MergeProperties { variable: String, value: Expr },
    /// SET n:Label
    Label { variable: String, label: String },
}

/// Schema commands (CREATE INDEX, CREATE CONSTRAINT, etc.)
#[derive(Debug, Clone)]
pub enum SchemaCommand {
    CreateIndex { label: String, property: String, index_type: Option<String> },
    DropIndex { label: String, property: String },
    CreateConstraint { label: String, property: String, constraint_type: String },
    DropConstraint { label: String, property: String },
}
