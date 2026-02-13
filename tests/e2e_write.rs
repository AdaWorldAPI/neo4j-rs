//! End-to-end integration tests for write operations.
//!
//! Tests CREATE, SET, DELETE operations via the full Cypher pipeline.
//! Each test exercises: parse -> plan -> optimize -> execute against MemoryBackend.

use neo4j_rs::{Graph, Node, PropertyMap, Value, StorageBackend, NodeId};

// ============================================================================
// 1. CREATE multiple nodes in one statement
// ============================================================================

#[tokio::test]
async fn test_create_multiple_nodes() {
    let graph = Graph::open_memory().await.unwrap();

    // CREATE two nodes in a single statement using comma-separated patterns
    graph
        .mutate(
            "CREATE (a:Person {name: 'Alice'}), (b:Person {name: 'Bob'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Count the Person nodes
    let result = graph
        .execute("MATCH (n:Person) RETURN count(n)", PropertyMap::new())
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 2);
}

// ============================================================================
// 2. CREATE node with multiple labels
// ============================================================================

#[tokio::test]
async fn test_create_node_multiple_labels() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person:Employee {name: 'Alice'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Should be findable by :Person label
    let result_person = graph
        .execute("MATCH (n:Person) RETURN n", PropertyMap::new())
        .await
        .unwrap();
    assert_eq!(result_person.rows.len(), 1);
    let node: Node = result_person.rows[0].get("n").unwrap();
    assert!(node.has_label("Person"));
    assert!(node.has_label("Employee"));

    // Should also be findable by :Employee label
    let result_employee = graph
        .execute("MATCH (n:Employee) RETURN n", PropertyMap::new())
        .await
        .unwrap();
    assert_eq!(result_employee.rows.len(), 1);
    let node2: Node = result_employee.rows[0].get("n").unwrap();
    assert_eq!(node2.get("name"), Some(&Value::String("Alice".into())));
}

// ============================================================================
// 3. SET property on existing node
// ============================================================================

#[tokio::test]
async fn test_set_single_property() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // SET a new property
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Alice' SET n.age = 30",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify the property was set
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = 'Alice' RETURN n.age",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(age, 30);
}

// ============================================================================
// 4. SET multiple properties (separate SET statements)
// ============================================================================

#[tokio::test]
async fn test_set_multiple_properties_separate() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // First SET: add age
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Alice' SET n.age = 30",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Second SET: add email
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Alice' SET n.email = 'alice@example.com'",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify both properties
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = 'Alice' RETURN n.age, n.email",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(age, 30);
    let email: String = result.rows[0].get("n.email").unwrap();
    assert_eq!(email, "alice@example.com");
}

// ============================================================================
// 5. SET overwrites existing property value
// ============================================================================

#[tokio::test]
async fn test_set_overwrite_property() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice', age: 25})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Overwrite age
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Alice' SET n.age = 30",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = 'Alice' RETURN n.age",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(age, 30, "Age should be overwritten from 25 to 30");
}

// ============================================================================
// 6. DELETE an unconnected node
// ============================================================================

#[tokio::test]
async fn test_delete_unconnected_node() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate("CREATE (n:Temp {val: 1})", PropertyMap::new())
        .await
        .unwrap();

    // Verify node exists
    let result = graph
        .execute("MATCH (n:Temp) RETURN count(n)", PropertyMap::new())
        .await
        .unwrap();
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 1);

    // DETACH DELETE the node (using DETACH DELETE even on unconnected nodes is safe)
    graph
        .mutate(
            "MATCH (n:Temp) DETACH DELETE n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify node is gone
    let result = graph
        .execute("MATCH (n:Temp) RETURN count(n)", PropertyMap::new())
        .await
        .unwrap();
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 0);
}

// ============================================================================
// 7. DETACH DELETE a connected node
// ============================================================================

#[tokio::test]
async fn test_detach_delete_connected_node() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new())
        .await
        .unwrap();
    graph
        .mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new())
        .await
        .unwrap();

    // Create relationship via backend API
    {
        let backend = graph.backend();
        let mut tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadWrite).await.unwrap();
        backend
            .create_relationship(&mut tx, NodeId(1), NodeId(2), "KNOWS", PropertyMap::new())
            .await
            .unwrap();
        backend.commit_tx(tx).await.unwrap();
    }

    // DETACH DELETE Alice (should remove Alice and the relationship)
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Alice' DETACH DELETE n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify Alice is gone
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = 'Alice' RETURN count(n)",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    let alice_count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(alice_count, 0, "Alice should be deleted");

    // Verify Bob still exists
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = 'Bob' RETURN count(n)",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    let bob_count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(bob_count, 1, "Bob should still exist");

    // Verify no relationships remain
    {
        let backend = graph.backend();
        let tx = backend.begin_tx(neo4j_rs::tx::TxMode::ReadOnly).await.unwrap();
        let rel_count = backend.relationship_count(&tx).await.unwrap();
        assert_eq!(rel_count, 0, "All relationships involving Alice should be deleted");
        backend.commit_tx(tx).await.unwrap();
    }
}

// ============================================================================
// 8. CREATE ... RETURN n (return created node)
// ============================================================================

#[tokio::test]
async fn test_create_and_return() {
    let graph = Graph::open_memory().await.unwrap();

    let result = graph
        .mutate(
            "CREATE (n:Person {name: 'Ada'}) RETURN n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.columns, vec!["n"]);
    assert_eq!(result.rows.len(), 1);

    let node: Node = result.rows[0].get("n").unwrap();
    assert!(node.has_label("Person"));
    assert_eq!(node.get("name"), Some(&Value::String("Ada".into())));
}

// ============================================================================
// 9. CREATE with RETURN property access
// ============================================================================

#[tokio::test]
async fn test_create_and_return_property() {
    let graph = Graph::open_memory().await.unwrap();

    let result = graph
        .mutate(
            "CREATE (n:Person {name: 'Ada', age: 3}) RETURN n.name",
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
// 10. DELETE specific node by property filter
// ============================================================================

#[tokio::test]
async fn test_delete_specific_node_by_filter() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    // Delete only Bob
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.name = 'Bob' DETACH DELETE n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify Bob is gone, Alice and Charlie remain
    let result = graph
        .execute("MATCH (n:Person) RETURN n.name", PropertyMap::new())
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Alice".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
    assert!(!names.contains(&"Bob".to_string()));
}

// ============================================================================
// 11. Create node with various property types
// ============================================================================

#[tokio::test]
async fn test_create_with_various_property_types() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Item {name: 'Widget', price: 9, active: true})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute("MATCH (n:Item) RETURN n", PropertyMap::new())
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let node: Node = result.rows[0].get("n").unwrap();
    assert_eq!(node.get("name"), Some(&Value::String("Widget".into())));
    assert_eq!(node.get("price"), Some(&Value::Int(9)));
    assert_eq!(node.get("active"), Some(&Value::Bool(true)));
}

// ============================================================================
// 12. SET property on multiple matching nodes
// ============================================================================

#[tokio::test]
async fn test_set_property_on_multiple_nodes() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice', dept: 'Engineering'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob', dept: 'Engineering'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie', dept: 'Marketing'})", PropertyMap::new()).await.unwrap();

    // SET reviewed=true on all Engineering people
    graph
        .mutate(
            "MATCH (n:Person) WHERE n.dept = 'Engineering' SET n.reviewed = true",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Verify Engineering nodes have reviewed=true
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.dept = 'Engineering' RETURN n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 2);
    for row in &result.rows {
        let node: Node = row.get("n").unwrap();
        assert_eq!(
            node.get("reviewed"),
            Some(&Value::Bool(true)),
            "Node {} should have reviewed=true",
            node.get("name").unwrap(),
        );
    }

    // Verify Marketing node does NOT have reviewed property
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.dept = 'Marketing' RETURN n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let charlie: Node = result.rows[0].get("n").unwrap();
    assert_eq!(charlie.get("reviewed"), None, "Charlie should not have reviewed property");
}

// ============================================================================
// 13. CREATE multiple nodes then count by label
// ============================================================================

#[tokio::test]
async fn test_create_different_labels_and_count() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Company {name: 'Acme'})", PropertyMap::new()).await.unwrap();

    let result = graph
        .execute("MATCH (n:Person) RETURN count(n)", PropertyMap::new())
        .await
        .unwrap();
    let person_count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(person_count, 2);

    let result = graph
        .execute("MATCH (n:Company) RETURN count(n)", PropertyMap::new())
        .await
        .unwrap();
    let company_count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(company_count, 1);
}
