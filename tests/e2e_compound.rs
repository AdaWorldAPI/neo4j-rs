//! Tests for compound Cypher statements: MATCH...CREATE, MATCH...MERGE
//!
//! These test the newly added ability to find nodes via MATCH and then
//! create relationships between them, which is fundamental Cypher usage:
//!
//!   MATCH (a:Person), (b:Person) WHERE a.name = 'Alice' AND b.name = 'Bob'
//!   CREATE (a)-[:KNOWS]->(b)

use neo4j_rs::{Graph, PropertyMap};

#[tokio::test]
async fn test_match_create_relationship() {
    let graph = Graph::open_memory().await.unwrap();

    // Create two nodes
    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    // Create relationship via MATCH...CREATE
    graph.mutate(
        "MATCH (a:Person), (b:Person) WHERE a.name = 'Alice' AND b.name = 'Bob' CREATE (a)-[:KNOWS]->(b)",
        PropertyMap::new(),
    ).await.unwrap();

    // Verify: traverse from Alice via KNOWS to find Bob
    let result = graph.execute(
        "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a.name, b.name",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1, "Expected 1 KNOWS relationship");
    let src = result.rows[0].get::<String>("a.name").unwrap();
    let dst = result.rows[0].get::<String>("b.name").unwrap();
    assert_eq!(src, "Alice");
    assert_eq!(dst, "Bob");
}

#[tokio::test]
async fn test_match_create_multiple_relationships() {
    let graph = Graph::open_memory().await.unwrap();

    // Create nodes
    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Company {name: 'Acme'})", PropertyMap::new()).await.unwrap();

    // Create two relationships
    graph.mutate(
        "MATCH (a:Person), (b:Person) WHERE a.name = 'Alice' AND b.name = 'Bob' CREATE (a)-[:KNOWS]->(b)",
        PropertyMap::new(),
    ).await.unwrap();

    graph.mutate(
        "MATCH (a:Person), (b:Company) WHERE a.name = 'Alice' AND b.name = 'Acme' CREATE (a)-[:WORKS_AT]->(b)",
        PropertyMap::new(),
    ).await.unwrap();

    // Verify Alice has 2 outgoing relationships
    let result = graph.execute(
        "MATCH (a:Person)-[r]->(b) WHERE a.name = 'Alice' RETURN b.name",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 2, "Expected 2 relationships from Alice");
}

#[tokio::test]
async fn test_match_create_with_where() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:System {name: 'Predator', type: 'UAV'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:System {name: 'Maven', type: 'AI'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Org {name: 'USAF'})", PropertyMap::new()).await.unwrap();

    // Only link UAV systems to the org
    graph.mutate(
        "MATCH (s:System), (o:Org) WHERE s.type = 'UAV' AND o.name = 'USAF' CREATE (s)-[:OPERATED_BY]->(o)",
        PropertyMap::new(),
    ).await.unwrap();

    let result = graph.execute(
        "MATCH (s:System)-[:OPERATED_BY]->(o:Org) RETURN s.name",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1, "Only Predator should be linked");
    assert_eq!(result.rows[0].get::<String>("s.name").unwrap(), "Predator");
}

#[tokio::test]
async fn test_standalone_create_still_works() {
    // Verify that standalone CREATE (no MATCH) still works after refactor
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate(
        "CREATE (n:Person {name: 'Charlie', age: 30})",
        PropertyMap::new(),
    ).await.unwrap();

    let result = graph.execute(
        "MATCH (n:Person) RETURN n.name, n.age",
        PropertyMap::new(),
    ).await.unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].get::<String>("n.name").unwrap(), "Charlie");
}

#[tokio::test]
async fn test_export_full_roundtrip_with_relationships() {
    // This test proves the full Cypher DUMP → reimport path works
    // now that MATCH...CREATE is supported
    let graph_a = Graph::open_memory().await.unwrap();

    // Create nodes
    graph_a.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph_a.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();

    // Create relationship via MATCH...CREATE
    graph_a.mutate(
        "MATCH (a:Person), (b:Person) WHERE a.name = 'Alice' AND b.name = 'Bob' CREATE (a)-[:KNOWS]->(b)",
        PropertyMap::new(),
    ).await.unwrap();

    // Export
    let mut buf = Vec::new();
    neo4j_rs::export::export_cypher_dump(graph_a.backend(), &mut buf)
        .await
        .unwrap();
    let dump = String::from_utf8(buf).unwrap();

    // Re-import into fresh graph
    let graph_b = Graph::open_memory().await.unwrap();
    for line in dump.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let stmt = trimmed.trim_end_matches(';');
        if stmt.is_empty() {
            continue;
        }
        graph_b.mutate(stmt, PropertyMap::new()).await
            .unwrap_or_else(|e| panic!("Failed to reimport: {}\nError: {}", stmt, e));
    }

    // Verify: reimported graph has same node count
    let result = graph_b.execute(
        "MATCH (n) RETURN count(n) AS cnt",
        PropertyMap::new(),
    ).await.unwrap();
    assert_eq!(result.rows[0].get::<i64>("cnt").unwrap(), 2, "Should have 2 nodes");
}
