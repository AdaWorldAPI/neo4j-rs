//! End-to-end test: AI War Cloud graph through neo4j-rs GUI layer.
//!
//! This test demonstrates the full Quadro pattern:
//! - User writes Cypher (same as Neo4j Browser)
//! - neo4j-rs parses, plans, executes
//! - MemoryBackend stores nodes/edges (LadybugBackend in production)
//! - Results come back as QueryResult rows
//!
//! The aiwar dataset maps AI decision-making systems from battlefield to desktop.

use neo4j_rs::{Graph, Value, PropertyMap};

/// Helper: create the aiwar graph in the given graph instance.
async fn seed_aiwar_graph(graph: &Graph<neo4j_rs::storage::MemoryBackend>) {
    // Systems
    let systems = vec![
        ("Predator", "UAV", "Intel", "Mapping"),
        ("Gorgon Stare", "IntelligentControlSystem", "Intel", "ObjectRecognition"),
        ("ARGUS-IS", "IntelligentControlSystem", "Intel", "ObjectDetection"),
        ("Maven", "GenerativeModel", "Intel", "Classification"),
        ("Palantir Gotham", "Dashboard", "Command", "PatternRecognition"),
        ("Clearview AI", "NarrowAI", "Personnel", "FaceRecognition"),
        ("Heron TP", "UAV", "Operations", "Mapping"),
        ("Iron Dome", "IntelligentControlSystem", "Prediction", "ObjectDetection"),
        ("ATLAS", "ServiceRobot", "Logistics", "Automate"),
    ];

    for (name, sys_type, military_use, capacity) in &systems {
        let mut params = PropertyMap::new();
        params.insert("name".into(), Value::from(*name));
        params.insert("type".into(), Value::from(*sys_type));
        params.insert("military_use".into(), Value::from(*military_use));
        params.insert("capacity".into(), Value::from(*capacity));
        graph.mutate(
            "CREATE (s:System {name: $name, type: $type, military_use: $military_use, capacity: $capacity})",
            params,
        ).await.expect("CREATE System failed");
    }

    // Stakeholders
    let stakeholders = vec![
        ("General Atomics", "DefenseCompany"),
        ("Lockheed Martin", "DefenseCompany"),
        ("Google", "TechCompany"),
        ("Palantir", "TechCompany"),
        ("Clearview AI Inc", "TechCompany"),
        ("IAI", "DefenseCompany"),
        ("Rafael", "DefenseCompany"),
        ("Boston Dynamics", "TechCompany"),
    ];

    for (name, st_type) in &stakeholders {
        let mut params = PropertyMap::new();
        params.insert("name".into(), Value::from(*name));
        params.insert("type".into(), Value::from(*st_type));
        graph.mutate(
            "CREATE (st:Stakeholder {name: $name, type: $type})",
            params,
        ).await.expect("CREATE Stakeholder failed");
    }
}

#[tokio::test]
async fn test_create_aiwar_nodes() {
    let graph = Graph::open_memory().await.unwrap();
    seed_aiwar_graph(&graph).await;

    // Query all systems
    let result = graph.execute(
        "MATCH (s:System) RETURN s",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 9, "Expected 9 systems");
}

#[tokio::test]
async fn test_query_systems_by_property() {
    let graph = Graph::open_memory().await.unwrap();
    seed_aiwar_graph(&graph).await;

    // Query systems with specific military_use
    let mut params = PropertyMap::new();
    params.insert("use".into(), Value::from("Intel"));
    let result = graph.execute(
        "MATCH (s:System) WHERE s.military_use = $use RETURN s.name, s.military_use",
        params,
    ).await.unwrap();

    // Predator, Gorgon Stare, ARGUS-IS, Maven all have military_use = Intel
    assert_eq!(result.rows.len(), 4, "Expected 4 Intel systems");
}

#[tokio::test]
async fn test_query_with_order_and_limit() {
    let graph = Graph::open_memory().await.unwrap();
    seed_aiwar_graph(&graph).await;

    let result = graph.execute(
        "MATCH (s:System) RETURN s.name ORDER BY s.name LIMIT 3",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 3, "Expected 3 rows with LIMIT 3");

    // Verify ordering (alphabetical): ARGUS-IS, ATLAS, Clearview AI
    let names: Vec<String> = result.rows.iter()
        .filter_map(|row| row.get::<String>("s.name").ok())
        .collect();
    assert_eq!(names[0], "ARGUS-IS");
    assert_eq!(names[1], "ATLAS");
}

#[tokio::test]
async fn test_aggregation_count_by_type() {
    let graph = Graph::open_memory().await.unwrap();
    seed_aiwar_graph(&graph).await;

    let result = graph.execute(
        "MATCH (s:System) RETURN s.type, count(s) AS cnt",
        PropertyMap::new(),
    ).await.unwrap();

    // Should have grouped rows (UAV: 2, IntelligentControlSystem: 3, etc.)
    assert!(result.rows.len() > 0, "Should have aggregation results");
}

#[tokio::test]
async fn test_stakeholder_labels() {
    let graph = Graph::open_memory().await.unwrap();
    seed_aiwar_graph(&graph).await;

    let result = graph.execute(
        "MATCH (st:Stakeholder) RETURN st.name, st.type",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 8, "Expected 8 stakeholders");
}

#[tokio::test]
async fn test_export_cypher_dump() {
    let graph = Graph::open_memory().await.unwrap();
    seed_aiwar_graph(&graph).await;

    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph.backend(), &mut buf)
        .await
        .unwrap();

    let dump = String::from_utf8(buf).unwrap();
    assert!(dump.contains("CREATE"), "Dump should contain CREATE statements");
    assert!(dump.contains("_id:"), "Dump should contain node IDs");
}

#[tokio::test]
async fn test_set_property() {
    let graph = Graph::open_memory().await.unwrap();

    // Create a system
    let mut params = PropertyMap::new();
    params.insert("name".into(), Value::from("TestDrone"));
    graph.mutate(
        "CREATE (s:System {name: $name})",
        params,
    ).await.unwrap();

    // Set a new property
    let mut params = PropertyMap::new();
    params.insert("value".into(), Value::from("Retired"));
    graph.mutate(
        "MATCH (s:System) WHERE s.name = 'TestDrone' SET s.status = $value",
        params,
    ).await.unwrap();

    // Verify
    let result = graph.execute(
        "MATCH (s:System) WHERE s.name = 'TestDrone' RETURN s.name, s.status",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1);
    let status = result.rows[0].get::<String>("s.status").ok();
    assert_eq!(status, Some("Retired".to_string()));
}

#[tokio::test]
async fn test_delete_node() {
    let graph = Graph::open_memory().await.unwrap();

    let mut params = PropertyMap::new();
    params.insert("name".into(), Value::from("Ephemeral"));
    graph.mutate(
        "CREATE (s:System {name: $name})",
        params,
    ).await.unwrap();

    // Verify it exists
    let result = graph.execute(
        "MATCH (s:System) WHERE s.name = 'Ephemeral' RETURN s",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows.len(), 1);

    // Delete it
    graph.mutate(
        "MATCH (s:System) WHERE s.name = 'Ephemeral' DELETE s",
        PropertyMap::new(),
    ).await.unwrap();

    // Verify gone
    let result = graph.execute(
        "MATCH (s:System) WHERE s.name = 'Ephemeral' RETURN s",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows.len(), 0);
}
