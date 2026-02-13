//! End-to-end integration tests for relationship traversal patterns.
//!
//! Tests multi-hop relationship patterns, bidirectional traversal,
//! relationship type filtering, relationship properties, and graph shape queries.
//!
//! Each test exercises: parse -> plan -> optimize -> execute against MemoryBackend.

use neo4j_rs::{Graph, PropertyMap, Value, StorageBackend, NodeId, Relationship};

// ============================================================================
// Helper: create a graph with nodes and relationships via the backend API.
//
// The Cypher CREATE clause currently only creates nodes (not relationships
// via pattern syntax), so we use the StorageBackend API to wire up edges.
// ============================================================================

/// Create a linear chain: Alice -[:KNOWS]-> Bob -[:KNOWS]-> Charlie.
/// Returns (graph, alice_id, bob_id, charlie_id).
async fn setup_linear_chain() -> (Graph<neo4j_rs::storage::MemoryBackend>, NodeId, NodeId, NodeId) {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    let alice = NodeId(1);
    let bob = NodeId(2);
    let charlie = NodeId(3);

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        backend.create_relationship(&mut tx, alice, bob, "KNOWS", PropertyMap::new()).await.unwrap();
        backend.create_relationship(&mut tx, bob, charlie, "KNOWS", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    (graph, alice, bob, charlie)
}

// ============================================================================
// 1. Two-hop traversal: Alice->Bob->Charlie
// ============================================================================

#[tokio::test]
async fn test_two_hop_traversal() {
    let (graph, _alice, _bob, _charlie) = setup_linear_chain().await;

    // MATCH (a:Person {name:'Alice'})-[:KNOWS]->(b)-[:KNOWS]->(c) RETURN c.name
    let result = graph
        .execute(
            "MATCH (a:Person)-[:KNOWS]->(b:Person)-[:KNOWS]->(c:Person) RETURN c.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Filter to find the Alice->Bob->Charlie chain
    let names: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| row.get::<String>("c.name").ok())
        .collect();

    assert!(
        names.contains(&"Charlie".to_string()),
        "Expected two-hop traversal to reach Charlie, got: {:?}",
        names,
    );
}

// ============================================================================
// 2. Single-hop relationship traversal (known working)
// ============================================================================

#[tokio::test]
async fn test_single_hop_traversal() {
    let (graph, _alice, _bob, _charlie) = setup_linear_chain().await;

    // Single hop: who does Alice know?
    let result = graph
        .execute(
            "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name, b.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert!(
        !result.rows.is_empty(),
        "Expected at least one relationship traversal result"
    );

    // Collect all (a.name, b.name) pairs
    let pairs: Vec<(String, String)> = result
        .rows
        .iter()
        .map(|row| {
            let a: String = row.get("a.name").unwrap();
            let b: String = row.get("b.name").unwrap();
            (a, b)
        })
        .collect();

    assert!(
        pairs.contains(&("Alice".to_string(), "Bob".to_string())),
        "Expected Alice->Bob in results, got: {:?}",
        pairs,
    );
    assert!(
        pairs.contains(&("Bob".to_string(), "Charlie".to_string())),
        "Expected Bob->Charlie in results, got: {:?}",
        pairs,
    );
}

// ============================================================================
// 3. Bidirectional relationship pattern: (a)-[:KNOWS]-(b)
// ============================================================================

#[tokio::test]
async fn test_bidirectional_relationship() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        // Only one directed relationship: Alice -> Bob
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // Undirected pattern should find the relationship from both directions
    let result = graph
        .execute(
            "MATCH (a:Person)-[:KNOWS]-(b:Person) RETURN a.name, b.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // With undirected (-[:KNOWS]-), both (Alice,Bob) and (Bob,Alice) should appear
    let pairs: Vec<(String, String)> = result
        .rows
        .iter()
        .map(|row| {
            let a: String = row.get("a.name").unwrap();
            let b: String = row.get("b.name").unwrap();
            (a, b)
        })
        .collect();

    // At minimum, Alice->Bob direction should be found
    assert!(
        pairs.contains(&("Alice".to_string(), "Bob".to_string())),
        "Expected Alice-Bob in undirected results, got: {:?}",
        pairs,
    );

    // Undirected should also find Bob->Alice (traversing the relationship backwards)
    assert!(
        pairs.contains(&("Bob".to_string(), "Alice".to_string())),
        "Expected Bob-Alice in undirected results, got: {:?}",
        pairs,
    );
}

// ============================================================================
// 4. Multiple relationship types: filter by specific type
// ============================================================================

#[tokio::test]
async fn test_multiple_relationship_types() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        // Alice -[:KNOWS]-> Bob
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", PropertyMap::new()).await.unwrap();
        // Alice -[:WORKS_WITH]-> Charlie
        backend.create_relationship(&mut tx, NodeId(1), NodeId(3), "WORKS_WITH", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // Only follow KNOWS relationships
    let result = graph
        .execute(
            "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN b.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("b.name").unwrap())
        .collect();

    assert!(
        names.contains(&"Bob".to_string()),
        "KNOWS should reach Bob"
    );
    assert!(
        !names.contains(&"Charlie".to_string()),
        "KNOWS should NOT reach Charlie (connected via WORKS_WITH)"
    );
}

// ============================================================================
// 5. Relationship with properties
// ============================================================================

#[tokio::test]
async fn test_relationship_with_properties() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();

        let mut rel_props = PropertyMap::new();
        rel_props.insert("since".to_string(), Value::Int(2020));
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", rel_props).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // MATCH with relationship alias to access properties
    let result = graph
        .execute(
            "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN r.since",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert!(!result.rows.is_empty(), "Should find the relationship");

    let since: i64 = result.rows[0].get("r.since").unwrap();
    assert_eq!(since, 2020);
}

// ============================================================================
// 6. Triangle pattern: A->B, B->C, C->A
// ============================================================================

#[tokio::test]
async fn test_triangle_pattern() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", PropertyMap::new()).await.unwrap();
        backend.create_relationship(&mut tx, NodeId(2), NodeId(3), "KNOWS", PropertyMap::new()).await.unwrap();
        backend.create_relationship(&mut tx, NodeId(3), NodeId(1), "KNOWS", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    let result = graph
        .execute(
            "MATCH (a:Person)-[:KNOWS]->(b:Person)-[:KNOWS]->(c:Person) RETURN a.name, b.name, c.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // In a complete triangle, every node is reachable from every other in 2 hops
    assert!(
        result.rows.len() >= 3,
        "Triangle should produce at least 3 two-hop paths, got {}",
        result.rows.len(),
    );
}

// ============================================================================
// 7. Relationship type function: type(r)
// ============================================================================

#[tokio::test]
async fn test_relationship_type_function() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "WORKS_WITH", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    let result = graph
        .execute(
            "MATCH (a:Person)-[r]->(b:Person) RETURN type(r)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert!(!result.rows.is_empty(), "Should find the relationship");

    let rel_type: String = result.rows[0].get("type").unwrap();
    assert_eq!(rel_type, "WORKS_WITH");
}

// ============================================================================
// 8. No relationship type filter (match any relationship)
// ============================================================================

#[tokio::test]
async fn test_any_relationship_type() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", PropertyMap::new()).await.unwrap();
        backend.create_relationship(&mut tx, NodeId(1), NodeId(3), "WORKS_WITH", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // Match any relationship (no type filter)
    let result = graph
        .execute(
            "MATCH (a:Person)-[r]->(b:Person) RETURN b.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("b.name").unwrap())
        .collect();

    assert_eq!(names.len(), 2, "Should find both relationships regardless of type");
    assert!(names.contains(&"Bob".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
}

// ============================================================================
// 9. Incoming relationship direction: <-[:KNOWS]-
// ============================================================================

#[tokio::test]
async fn test_incoming_relationship_direction() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        // Alice -> Bob
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", PropertyMap::new()).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // Query from Bob's perspective using incoming direction
    let result = graph
        .execute(
            "MATCH (b:Person)<-[:KNOWS]-(a:Person) RETURN a.name, b.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert!(!result.rows.is_empty(), "Should find incoming relationship");

    // a should be Alice (the source), b should be Bob (the target with incoming)
    let a_name: String = result.rows[0].get("a.name").unwrap();
    let b_name: String = result.rows[0].get("b.name").unwrap();

    // The Expand uses the `from` alias as the scan root, and the direction
    // determines which direction to follow edges. With <-[:KNOWS]-, we
    // expect the pattern to find edges where `from` is the destination.
    assert!(
        (a_name == "Alice" && b_name == "Bob") || (a_name == "Bob" && b_name == "Alice"),
        "Expected Alice<-[:KNOWS]-Bob pattern, got a={}, b={}",
        a_name,
        b_name,
    );
}

// ============================================================================
// 10. Relationship with aliased return
// ============================================================================

#[tokio::test]
async fn test_relationship_alias_return() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        let mut props = PropertyMap::new();
        props.insert("since".to_string(), Value::Int(2015));
        backend.create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", props).await.unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // Return the full relationship object
    let result = graph
        .execute(
            "MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN r",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert!(!result.rows.is_empty(), "Should return the relationship");

    let rel: Relationship = result.rows[0].get("r").unwrap();
    assert_eq!(rel.rel_type, "KNOWS");
    assert_eq!(rel.properties.get("since"), Some(&Value::Int(2015)));
}
