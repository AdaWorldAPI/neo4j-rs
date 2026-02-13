//! Universal value type matching Neo4j's type system.

use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Node, Relationship, Path};

/// Neo4j-compatible value type.
///
/// Covers all types in the Neo4j type system:
/// - Scalars: Bool, Int, Float, String, Bytes
/// - Containers: List, Map
/// - Graph: Node, Relationship, Path
/// - Temporal: Date, Time, DateTime, LocalDateTime, Duration
/// - Spatial: Point2D, Point3D
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(HashMap<String, Value>),

    // Graph types
    Node(Box<Node>),
    Relationship(Box<Relationship>),
    Path(Box<Path>),

    // Temporal types
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(DateTime<Utc>),
    LocalDateTime(NaiveDateTime),
    Duration(IsoDuration),

    // Spatial types
    Point2D { srid: i32, x: f64, y: f64 },
    Point3D { srid: i32, x: f64, y: f64, z: f64 },
}

/// ISO 8601 duration (months, days, seconds, nanoseconds)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IsoDuration {
    pub months: i64,
    pub days: i64,
    pub seconds: i64,
    pub nanoseconds: i32,
}

// ============================================================================
// Type checking
// ============================================================================

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NULL",
            Value::Bool(_) => "BOOLEAN",
            Value::Int(_) => "INTEGER",
            Value::Float(_) => "FLOAT",
            Value::String(_) => "STRING",
            Value::Bytes(_) => "BYTES",
            Value::List(_) => "LIST",
            Value::Map(_) => "MAP",
            Value::Node(_) => "NODE",
            Value::Relationship(_) => "RELATIONSHIP",
            Value::Path(_) => "PATH",
            Value::Date(_) => "DATE",
            Value::Time(_) => "TIME",
            Value::DateTime(_) => "DATETIME",
            Value::LocalDateTime(_) => "LOCAL_DATETIME",
            Value::Duration(_) => "DURATION",
            Value::Point2D { .. } => "POINT",
            Value::Point3D { .. } => "POINT",
        }
    }

    pub fn is_null(&self) -> bool { matches!(self, Value::Null) }
    pub fn is_numeric(&self) -> bool { matches!(self, Value::Int(_) | Value::Float(_)) }
    pub fn is_string(&self) -> bool { matches!(self, Value::String(_)) }

    /// Neo4j-compatible truthiness
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            _ => true,
        }
    }

    /// Attempt to extract as i64
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }

    /// Attempt to extract as f64
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Attempt to extract as &str
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

// ============================================================================
// Conversions (From impls)
// ============================================================================

impl From<bool> for Value { fn from(v: bool) -> Self { Value::Bool(v) } }
impl From<i32> for Value { fn from(v: i32) -> Self { Value::Int(v as i64) } }
impl From<i64> for Value { fn from(v: i64) -> Self { Value::Int(v) } }
impl From<f64> for Value { fn from(v: f64) -> Self { Value::Float(v) } }
impl From<String> for Value { fn from(v: String) -> Self { Value::String(v) } }
impl From<&str> for Value { fn from(v: &str) -> Self { Value::String(v.to_owned()) } }
impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self { Value::List(v.into_iter().map(Into::into).collect()) }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self { v.map(Into::into).unwrap_or(Value::Null) }
}

// ============================================================================
// Display
// ============================================================================

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(v) => write!(f, "{v}"),
            Value::String(s) => write!(f, "\"{}\"", s.replace('"', "\\\"")),
            Value::Bytes(b) => write!(f, "<bytes[{}]>", b.len()),
            Value::List(l) => {
                write!(f, "[")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{v}")?;
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                write!(f, "{{")?;
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Value::Node(n) => write!(f, "{n:?}"),
            Value::Relationship(r) => write!(f, "{r:?}"),
            Value::Path(p) => write!(f, "{p:?}"),
            Value::Date(d) => write!(f, "{d}"),
            Value::Time(t) => write!(f, "{t}"),
            Value::DateTime(dt) => write!(f, "{dt}"),
            Value::LocalDateTime(dt) => write!(f, "{dt}"),
            Value::Duration(d) => write!(f, "P{}M{}DT{}S", d.months, d.days, d.seconds),
            Value::Point2D { x, y, srid } => write!(f, "point({{srid: {srid}, x: {x}, y: {y}}})"),
            Value::Point3D { x, y, z, srid } => write!(f, "point({{srid: {srid}, x: {x}, y: {y}, z: {z}}})"),
        }
    }
}

// ============================================================================
// Comparison (Neo4j ordering rules)
// ============================================================================

impl Value {
    /// Neo4j comparison. Returns None for incompatible types (like SQL NULL behavior).
    pub fn neo4j_cmp(&self, other: &Value) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        match (self, other) {
            (Value::Null, Value::Null) => None, // NULL = NULL is NULL in Neo4j
            (Value::Null, _) | (_, Value::Null) => None,
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
            (Value::String(a), Value::String(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_from() {
        assert_eq!(Value::from("hello"), Value::String("hello".into()));
        assert_eq!(Value::from(42), Value::Int(42));
        assert_eq!(Value::from(3.14), Value::Float(3.14));
        assert_eq!(Value::from(true), Value::Bool(true));
    }

    #[test]
    fn test_null_comparison() {
        assert_eq!(Value::Null.neo4j_cmp(&Value::Null), None);
        assert_eq!(Value::Null.neo4j_cmp(&Value::Int(1)), None);
    }

    #[test]
    fn test_numeric_comparison() {
        assert_eq!(
            Value::Int(1).neo4j_cmp(&Value::Float(1.5)),
            Some(std::cmp::Ordering::Less)
        );
    }
}
