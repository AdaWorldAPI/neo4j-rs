//! End-to-end integration tests for the full Cypher pipeline.
//!
//! Each test exercises: parse -> plan -> optimize -> execute against MemoryBackend.
//! These tests use `Graph::execute()` for reads and `Graph::mutate()` for writes.

use neo4j_rs::{Graph, Node, PropertyMap, Value};

// ============================================================================
// 1. CREATE a node, then MATCH it back
// ============================================================================

#[tokio::test]
async fn test_create_and_query_node() {
    let graph = Graph::open_memory().await.unwrap();

    // CREATE a Person node
    graph
        .mutate("CREATE (n:Person)", PropertyMap::new())
        .await
        .unwrap();

    // MATCH it back
    let result = graph
        .execute("MATCH (n:Person) RETURN n", PropertyMap::new())
        .await
        .unwrap();

    assert_eq!(result.columns, vec!["n"]);
    assert_eq!(result.rows.len(), 1);

    let node: Node = result.rows[0].get("n").unwrap();
    assert!(node.has_label("Person"));
}

// ============================================================================
// 2. CREATE with properties, query back properties
// ============================================================================

#[tokio::test]
async fn test_create_with_properties() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada', age: 3})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute("MATCH (n:Person) RETURN n", PropertyMap::new())
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let node: Node = result.rows[0].get("n").unwrap();
    assert_eq!(node.get("name"), Some(&Value::String("Ada".into())));
    assert_eq!(node.get("age"), Some(&Value::Int(3)));
}

// ============================================================================
// 3. Create multiple nodes, filter by property with WHERE
// ============================================================================

#[tokio::test]
async fn test_match_with_where_filter() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada', age: 3})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Bob', age: 30})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Charlie', age: 25})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Filter for age > 10
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.age > 10 RETURN n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 2);

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| {
            let node: Node = row.get("n").unwrap();
            node.get("name").unwrap().as_str().unwrap().to_string()
        })
        .collect();

    assert!(names.contains(&"Bob".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
    assert!(!names.contains(&"Ada".to_string()));
}

// ============================================================================
// 4. RETURN property access: n.name
// ============================================================================

#[tokio::test]
async fn test_return_property_access() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada', age: 3})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.columns, vec!["n.name"]);
    assert_eq!(result.rows.len(), 1);

    let name: String = result.rows[0].get("n.name").unwrap();
    assert_eq!(name, "Ada");
}

// ============================================================================
// 5. count(n) aggregate
// ============================================================================

#[tokio::test]
async fn test_count_aggregate() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Bob'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Charlie'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN count(n)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 3);
}

// ============================================================================
// 6. RETURN with LIMIT
// ============================================================================

#[tokio::test]
async fn test_match_with_limit() {
    let graph = Graph::open_memory().await.unwrap();

    for i in 0..5 {
        graph
            .mutate(
                &format!("CREATE (n:Person {{name: 'Person{}'}})", i),
                PropertyMap::new(),
            )
            .await
            .unwrap();
    }

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n LIMIT 2",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 2);
}

// ============================================================================
// 7. CREATE nodes and relationship, traverse
// ============================================================================

#[tokio::test]
async fn test_create_and_match_relationship() {
    let graph = Graph::open_memory().await.unwrap();

    // Create two nodes with distinct names
    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Bob'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Create relationship using the backend directly since
    // CREATE ()-[]->() with variable bindings requires multi-clause support.
    // Instead, we use the backend to create the relationship,
    // then test that MATCH with relationship pattern works.
    {
        use neo4j_rs::StorageBackend;
        let backend = graph.backend();
        let mut tx = backend
            .begin_tx(neo4j_rs::tx::TxMode::ReadWrite)
            .await
            .unwrap();
        // Node IDs are 1-based in MemoryBackend
        use neo4j_rs::NodeId;
        backend
            .create_relationship(
                &mut tx,
                NodeId(1),
                NodeId(2),
                "KNOWS",
                PropertyMap::new(),
            )
            .await
            .unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // Now MATCH the relationship pattern
    let result = graph
        .execute(
            "MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a, b",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Should find at least one pair
    assert!(
        !result.rows.is_empty(),
        "Expected at least one relationship traversal result"
    );

    // Check we got nodes back
    let a_node: Node = result.rows[0].get("a").unwrap();
    let b_node: Node = result.rows[0].get("b").unwrap();
    assert!(a_node.has_label("Person"));
    assert!(b_node.has_label("Person"));

    // Verify the relationship direction: Ada -> Bob
    let a_name = a_node.get("name").unwrap().as_str().unwrap();
    let b_name = b_node.get("name").unwrap().as_str().unwrap();
    assert_eq!(a_name, "Ada");
    assert_eq!(b_name, "Bob");
}

// ============================================================================
// 8. MATCH node, SET property, verify change
// ============================================================================

#[tokio::test]
async fn test_match_set_property() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada', age: 3})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // SET the age property to 4
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Ada' SET n.age = 4",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify the change
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = 'Ada' RETURN n.age",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(age, 4);
}

// ============================================================================
// 9. Create nodes, DETACH DELETE one, verify gone
// ============================================================================

#[tokio::test]
async fn test_match_delete_node() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Bob'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Delete Ada
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Ada' DETACH DELETE n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify only Bob remains
    let result = graph
        .execute("MATCH (n:Person) RETURN n", PropertyMap::new())
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let node: Node = result.rows[0].get("n").unwrap();
    assert_eq!(
        node.get("name"),
        Some(&Value::String("Bob".into()))
    );
}

// ============================================================================
// 10. RETURN multiple columns: n.name, n.age
// ============================================================================

#[tokio::test]
async fn test_return_multiple_columns() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Ada', age: 3})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name, n.age",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.columns.len(), 2);
    assert!(result.columns.contains(&"n.name".to_string()));
    assert!(result.columns.contains(&"n.age".to_string()));

    assert_eq!(result.rows.len(), 1);

    let name: String = result.rows[0].get("n.name").unwrap();
    assert_eq!(name, "Ada");

    let age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(age, 3);
}
