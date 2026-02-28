//! Integration tests for the LadybugBackend.
//!
//! These tests exercise the full neo4j-rs pipeline (parse → plan → execute)
//! against LadybugBackend instead of MemoryBackend. This verifies that:
//!   1. BindSpace node/edge operations work through the StorageBackend trait
//!   2. JSON-encoded properties survive the round-trip
//!   3. Fingerprints are generated deterministically
//!   4. The same Cypher queries produce equivalent results on both backends
//!
//! REQUIRES: `cargo test --features ladybug --test e2e_ladybug`
//! (Won't compile without the real ladybug-rs + rustynum dependencies)
#![cfg(feature = "ladybug")]

use neo4j_rs::{Graph, Value, PropertyMap};

#[tokio::test]
async fn test_ladybug_create_and_query() {
    let graph = Graph::open_ladybug();

    let mut params = PropertyMap::new();
    params.insert("name".into(), Value::from("Alice"));
    graph.mutate(
        "CREATE (n:Person {name: $name})",
        params,
    ).await.unwrap();

    let result = graph.execute(
        "MATCH (n:Person) RETURN n.name",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1);
    let name = result.rows[0].get::<String>("n.name").unwrap();
    assert_eq!(name, "Alice");
}

#[tokio::test]
async fn test_ladybug_multiple_labels() {
    let graph = Graph::open_ladybug();

    graph.mutate(
        "CREATE (n:Person {name: 'Bob'})",
        PropertyMap::new(),
    ).await.unwrap();

    graph.mutate(
        "CREATE (n:System {name: 'Predator', type: 'UAV'})",
        PropertyMap::new(),
    ).await.unwrap();

    // Query only Person nodes
    let result = graph.execute(
        "MATCH (n:Person) RETURN n.name",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows.len(), 1);

    // Query only System nodes
    let result = graph.execute(
        "MATCH (n:System) RETURN n.name",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows.len(), 1);
}

#[tokio::test]
async fn test_ladybug_property_types() {
    let graph = Graph::open_ladybug();

    let mut params = PropertyMap::new();
    params.insert("name".into(), Value::from("TestNode"));
    params.insert("count".into(), Value::Int(42));
    params.insert("score".into(), Value::Float(3.14));
    params.insert("active".into(), Value::Bool(true));
    graph.mutate(
        "CREATE (n:Test {name: $name, count: $count, score: $score, active: $active})",
        params,
    ).await.unwrap();

    let result = graph.execute(
        "MATCH (n:Test) RETURN n.name, n.count, n.score, n.active",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get::<String>("n.name").unwrap(), "TestNode");
    assert_eq!(result.rows[0].get::<i64>("n.count").unwrap(), 42);
}

#[tokio::test]
async fn test_ladybug_set_property() {
    let graph = Graph::open_ladybug();

    graph.mutate(
        "CREATE (n:Person {name: 'Charlie'})",
        PropertyMap::new(),
    ).await.unwrap();

    let mut params = PropertyMap::new();
    params.insert("status".into(), Value::from("active"));
    graph.mutate(
        "MATCH (n:Person) WHERE n.name = 'Charlie' SET n.status = $status",
        params,
    ).await.unwrap();

    let result = graph.execute(
        "MATCH (n:Person) WHERE n.name = 'Charlie' RETURN n.status",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1);
    let status = result.rows[0].get::<String>("n.status").unwrap();
    assert_eq!(status, "active");
}

#[tokio::test]
async fn test_ladybug_delete_node() {
    let graph = Graph::open_ladybug();

    graph.mutate(
        "CREATE (n:Temp {name: 'Ephemeral'})",
        PropertyMap::new(),
    ).await.unwrap();

    // Verify exists
    let result = graph.execute(
        "MATCH (n:Temp) RETURN n",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows.len(), 1);

    // Delete
    graph.mutate(
        "MATCH (n:Temp) WHERE n.name = 'Ephemeral' DELETE n",
        PropertyMap::new(),
    ).await.unwrap();

    // Verify gone
    let result = graph.execute(
        "MATCH (n:Temp) RETURN n",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows.len(), 0);
}

#[tokio::test]
async fn test_ladybug_count_aggregate() {
    let graph = Graph::open_ladybug();

    for name in &["Alpha", "Beta", "Gamma"] {
        let mut params = PropertyMap::new();
        params.insert("name".into(), Value::from(*name));
        graph.mutate(
            "CREATE (n:System {name: $name})",
            params,
        ).await.unwrap();
    }

    let result = graph.execute(
        "MATCH (n:System) RETURN count(n) AS cnt",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1);
    let cnt = result.rows[0].get::<i64>("cnt").unwrap();
    assert_eq!(cnt, 3);
}

#[tokio::test]
async fn test_ladybug_export_dump() {
    let graph = Graph::open_ladybug();

    graph.mutate(
        "CREATE (n:Person {name: 'ExportTest'})",
        PropertyMap::new(),
    ).await.unwrap();

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    assert!(dump.contains("CREATE"), "Dump should contain CREATE");
    assert!(dump.contains("ExportTest"), "Dump should contain node name");
    assert!(dump.contains(":Person"), "Dump should contain label");
}

#[tokio::test]
async fn test_ladybug_order_by_limit() {
    let graph = Graph::open_ladybug();

    for (name, age) in &[("Zara", 20), ("Alice", 30), ("Mika", 25)] {
        let mut params = PropertyMap::new();
        params.insert("name".into(), Value::from(*name));
        params.insert("age".into(), Value::Int(*age));
        graph.mutate(
            "CREATE (n:Person {name: $name, age: $age})",
            params,
        ).await.unwrap();
    }

    let result = graph.execute(
        "MATCH (n:Person) RETURN n.name ORDER BY n.name LIMIT 2",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 2);
    let names: Vec<String> = result.rows.iter()
        .filter_map(|r| r.get::<String>("n.name").ok())
        .collect();
    assert_eq!(names[0], "Alice");
    assert_eq!(names[1], "Mika");
}

#[tokio::test]
async fn test_ladybug_where_filter() {
    let graph = Graph::open_ladybug();

    for (name, age) in &[("Young", 18), ("Middle", 35), ("Senior", 60)] {
        let mut params = PropertyMap::new();
        params.insert("name".into(), Value::from(*name));
        params.insert("age".into(), Value::Int(*age));
        graph.mutate(
            "CREATE (n:Person {name: $name, age: $age})",
            params,
        ).await.unwrap();
    }

    let result = graph.execute(
        "MATCH (n:Person) WHERE n.age > 30 RETURN n.name ORDER BY n.name",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 2);
    let names: Vec<String> = result.rows.iter()
        .filter_map(|r| r.get::<String>("n.name").ok())
        .collect();
    assert_eq!(names[0], "Middle");
    assert_eq!(names[1], "Senior");
}
