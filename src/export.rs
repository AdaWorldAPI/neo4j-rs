//! Cypher DUMP export — serialize a graph as Cypher statements.
//!
//! Produces a Cypher script that can be loaded into Neo4j Aura or any
//! Neo4j-compatible database. This is the migration path from ladybug-rs
//! back to Neo4j if the user wants it.
//!
//! ```text
//! neo4j-rs Graph → export_cypher_dump() → CREATE/MERGE statements
//!   → pipe into neo4j-admin import, or paste into Neo4j Browser
//! ```

use std::io::Write;
use crate::model::*;
use crate::storage::StorageBackend;
use crate::tx::TxMode;
use crate::Result;

/// Export a graph as a Cypher DUMP script.
///
/// Writes CREATE statements for all nodes and relationships in the graph.
/// The output can be loaded into Neo4j Aura, Neo4j Browser, or any
/// Cypher-compatible system.
pub async fn export_cypher_dump<B: StorageBackend>(
    backend: &B,
    writer: &mut dyn Write,
) -> Result<()> {
    let mut tx = backend.begin_tx(TxMode::ReadOnly).await?;

    // Header
    writeln!(writer, "// neo4j-rs Cypher DUMP")?;
    writeln!(writer, "// Generated for Neo4j Aura import")?;
    writeln!(writer, "// Nodes: {}", backend.node_count(&mut tx).await?)?;
    writeln!(writer, "// Relationships: {}", backend.relationship_count(&mut tx).await?)?;
    writeln!(writer)?;

    // Export all nodes
    let nodes = backend.all_nodes(&mut tx).await?;
    for node in &nodes {
        let labels_str = if node.labels.is_empty() {
            String::new()
        } else {
            format!(":{}", node.labels.join(":"))
        };

        let props_str = format_properties(&node.properties);

        writeln!(
            writer,
            "CREATE (n{} {{_id: {}{}}});",
            labels_str,
            node.id.0,
            if props_str.is_empty() { String::new() } else { format!(", {}", props_str) }
        )?;
    }

    writeln!(writer)?;
    writeln!(writer, "// Relationships")?;

    // Export all relationships
    for node in &nodes {
        let rels = backend.get_relationships(
            &mut tx,
            node.id,
            Direction::Outgoing,
            None,
        ).await?;

        for rel in rels {
            let props_str = format_properties(&rel.properties);
            let props_part = if props_str.is_empty() {
                String::new()
            } else {
                format!(" {{{}}}", props_str)
            };

            writeln!(
                writer,
                "MATCH (a {{_id: {}}}), (b {{_id: {}}}) CREATE (a)-[:{}{}]->(b);",
                rel.start_node_id.0,
                rel.end_node_id.0,
                rel.rel_type,
                props_part,
            )?;
        }
    }

    backend.commit_tx(tx).await?;
    Ok(())
}

/// Format a PropertyMap as Cypher property string (key: value, ...).
fn format_properties(props: &PropertyMap) -> String {
    let mut parts = Vec::new();
    for (key, value) in props.iter() {
        // Skip internal properties
        if key.starts_with('_') {
            continue;
        }
        parts.push(format!("{}: {}", key, format_value(value)));
    }
    parts.join(", ")
}

/// Format a Value as a Cypher literal.
fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => format!("{}", f),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(format_value).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Map(m) => {
            let inner: Vec<String> = m.iter()
                .map(|(k, v)| format!("{}: {}", k, format_value(v)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        _ => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_value() {
        assert_eq!(format_value(&Value::String("hello".into())), "'hello'");
        assert_eq!(format_value(&Value::Int(42)), "42");
        assert_eq!(format_value(&Value::Float(3.14)), "3.14");
        assert_eq!(format_value(&Value::Bool(true)), "true");
        assert_eq!(format_value(&Value::Null), "null");
    }

    #[test]
    fn test_format_properties() {
        let mut props = PropertyMap::new();
        props.insert("name".into(), Value::String("Ada".into()));
        props.insert("age".into(), Value::Int(3));
        let result = format_properties(&props);
        assert!(result.contains("name: 'Ada'"));
        assert!(result.contains("age: 3"));
    }
}
