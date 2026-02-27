//! Export round-trip test: dump a graph → verify the dump → re-import nodes.
//!
//! Tests the Neo4j Aura migration path:
//!   Graph A → export_cypher_dump() → Cypher script → verify correctness
//!
//! NOTE: Full round-trip (re-import relationships) requires MATCH...CREATE compound
//! statement support in the parser. For now, we verify the dump format is correct
//! and that node CREATE statements can be re-imported.

use neo4j_rs::{Graph, Value, PropertyMap};
use neo4j_rs::storage::StorageBackend;
use neo4j_rs::tx::TxMode;

/// Helper: create a test graph with nodes and relationships via backend API.
async fn seed_graph(graph: &Graph<neo4j_rs::storage::MemoryBackend>) {
    let backend = graph.backend();
    let mut tx = backend.begin_tx(TxMode::ReadWrite).await.unwrap();

    // Create 4 nodes
    let alice = backend.create_node(&mut tx, &["Person"], {
        let mut p = PropertyMap::new();
        p.insert("name".into(), Value::from("Alice"));
        p.insert("age".into(), Value::Int(30));
        p
    }).await.unwrap();

    let bob = backend.create_node(&mut tx, &["Person"], {
        let mut p = PropertyMap::new();
        p.insert("name".into(), Value::from("Bob"));
        p.insert("age".into(), Value::Int(25));
        p
    }).await.unwrap();

    let charlie = backend.create_node(&mut tx, &["Person"], {
        let mut p = PropertyMap::new();
        p.insert("name".into(), Value::from("Charlie"));
        p.insert("age".into(), Value::Int(35));
        p
    }).await.unwrap();

    let acme = backend.create_node(&mut tx, &["Company"], {
        let mut p = PropertyMap::new();
        p.insert("name".into(), Value::from("Acme"));
        p.insert("employees".into(), Value::Int(100));
        p
    }).await.unwrap();

    // Create 3 relationships
    backend.create_relationship(&mut tx, alice, bob, "KNOWS", PropertyMap::new()).await.unwrap();
    backend.create_relationship(&mut tx, bob, charlie, "KNOWS", PropertyMap::new()).await.unwrap();
    backend.create_relationship(&mut tx, alice, acme, "WORKS_AT", PropertyMap::new()).await.unwrap();

    backend.commit_tx(tx).await.unwrap();
}

#[tokio::test]
async fn test_export_node_count() {
    let graph = Graph::open_memory().await.unwrap();
    seed_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Verify dump has CREATE statements for all 4 nodes
    let create_count = dump.lines()
        .filter(|l| l.starts_with("CREATE"))
        .count();
    assert_eq!(create_count, 4, "Expected 4 CREATE node statements, got {}", create_count);
}

#[tokio::test]
async fn test_export_relationship_count() {
    let graph = Graph::open_memory().await.unwrap();
    seed_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Verify dump has MATCH...CREATE for all 3 relationships
    let match_create_count = dump.lines()
        .filter(|l| l.starts_with("MATCH"))
        .count();
    assert_eq!(match_create_count, 3, "Expected 3 relationship statements, got {}", match_create_count);
}

#[tokio::test]
async fn test_export_properties_preserved() {
    let graph = Graph::open_memory().await.unwrap();
    seed_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Verify key properties appear in the dump
    assert!(dump.contains("Alice"), "Dump should contain 'Alice'");
    assert!(dump.contains("Bob"), "Dump should contain 'Bob'");
    assert!(dump.contains("Charlie"), "Dump should contain 'Charlie'");
    assert!(dump.contains("Acme"), "Dump should contain 'Acme'");

    // Verify relationship types
    assert!(dump.contains("KNOWS"), "Dump should contain KNOWS relationship type");
    assert!(dump.contains("WORKS_AT"), "Dump should contain WORKS_AT relationship type");
}

#[tokio::test]
async fn test_export_header_contains_counts() {
    let graph = Graph::open_memory().await.unwrap();
    seed_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Header should report correct counts
    assert!(dump.contains("// Nodes: 4"), "Header should report 4 nodes, got:\n{}",
        dump.lines().take(5).collect::<Vec<_>>().join("\n"));
    assert!(dump.contains("// Relationships: 3"), "Header should report 3 relationships");
}

#[tokio::test]
async fn test_export_empty_graph() {
    let graph = Graph::open_memory().await.unwrap();

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    assert!(dump.contains("// Nodes: 0"), "Empty graph should report 0 nodes");
    assert!(dump.contains("// Relationships: 0"), "Empty graph should report 0 rels");

    // No CREATE statements
    let create_count = dump.lines()
        .filter(|l| l.starts_with("CREATE") || l.starts_with("MATCH"))
        .count();
    assert_eq!(create_count, 0, "Empty graph should have no CREATE/MATCH statements");
}

#[tokio::test]
async fn test_export_node_reimport() {
    let graph_a = Graph::open_memory().await.unwrap();
    seed_graph(&graph_a).await;

    // Export
    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph_a.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Re-import just the CREATE node statements into a fresh graph
    let graph_b = Graph::open_memory().await.unwrap();
    for line in dump.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("CREATE") {
            continue;
        }
        let stmt = trimmed.trim_end_matches(';');
        graph_b.mutate(stmt, PropertyMap::new()).await
            .unwrap_or_else(|e| panic!("Failed to import: {}\nError: {}", stmt, e));
    }

    // Verify same number of nodes
    let result = graph_b.execute(
        "MATCH (n) RETURN count(n) AS cnt",
        PropertyMap::new(),
    ).await.unwrap();
    let count = result.rows[0].get::<i64>("cnt").unwrap();
    assert_eq!(count, 4, "Reimported graph should have 4 nodes, got {}", count);
}

#[tokio::test]
async fn test_export_labels_preserved() {
    let graph = Graph::open_memory().await.unwrap();
    seed_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Verify labels appear in CREATE statements
    assert!(dump.contains(":Person"), "Dump should contain :Person label");
    assert!(dump.contains(":Company"), "Dump should contain :Company label");
}

#[tokio::test]
async fn test_export_relationship_endpoints() {
    let graph = Graph::open_memory().await.unwrap();
    seed_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Every MATCH line should reference two _id values and create a relationship
    let rel_lines: Vec<&str> = dump.lines()
        .filter(|l| l.starts_with("MATCH"))
        .collect();

    for line in &rel_lines {
        assert!(line.contains("_id:"), "Relationship line should reference _id: {}", line);
        assert!(line.contains("CREATE"), "Relationship line should contain CREATE: {}", line);
        assert!(line.contains("-["), "Relationship line should have relationship pattern: {}", line);
        assert!(line.contains("]->"), "Relationship line should have directed edge: {}", line);
    }
}
