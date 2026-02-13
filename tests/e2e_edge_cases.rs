//! End-to-end integration tests for edge cases and advanced expressions.
//!
//! Tests null handling, string operators, boolean logic, type coercion,
//! parameter substitution, arithmetic, CASE expressions, and more.
//! Each test exercises: parse -> plan -> optimize -> execute against MemoryBackend.

use neo4j_rs::{Graph, PropertyMap, Value};

// ============================================================================
// Helper: create a set of Person nodes with names and ages.
// ============================================================================

async fn setup_people() -> Graph<neo4j_rs::storage::MemoryBackend> {
    let graph = Graph::open_memory().await.unwrap();

    let people = [
        ("Alice", 25),
        ("Bob", 30),
        ("Charlie", 35),
        ("Diana", 28),
        ("Eve", 22),
    ];

    for (name, age) in &people {
        graph
            .mutate(
                &format!("CREATE (n:Person {{name: '{}', age: {}}})", name, age),
                PropertyMap::new(),
            )
            .await
            .unwrap();
    }

    graph
}

// ============================================================================
// 1. Null property access: missing property returns Value::Null
// ============================================================================

#[tokio::test]
async fn test_null_property_access() {
    let graph = Graph::open_memory().await.unwrap();

    // Create a node without an 'age' property
    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Access the missing 'age' property
    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.age",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let val: Value = result.rows[0].get("n.age").unwrap();
    assert_eq!(val, Value::Null, "Missing property should return Null");
}

// ============================================================================
// 2. Empty result: MATCH on non-existent label
// ============================================================================

#[tokio::test]
async fn test_empty_result() {
    let graph = Graph::open_memory().await.unwrap();

    let result = graph
        .execute(
            "MATCH (n:NonExistent) RETURN n",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 0, "Non-existent label should return 0 rows");
}

// ============================================================================
// 3. IS NULL filter
// ============================================================================

#[tokio::test]
async fn test_is_null_filter() {
    let graph = Graph::open_memory().await.unwrap();

    // Create nodes: Alice has email, Bob does not
    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice', email: 'alice@example.com'})",
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

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.email IS NULL RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Bob should have NULL email");
    assert_eq!(names[0], "Bob");
}

// ============================================================================
// 4. IS NOT NULL filter
// ============================================================================

#[tokio::test]
async fn test_is_not_null_filter() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice', email: 'alice@example.com'})",
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

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.email IS NOT NULL RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Alice should have non-NULL email");
    assert_eq!(names[0], "Alice");
}

// ============================================================================
// 5. String STARTS WITH
// ============================================================================

#[tokio::test]
#[ignore = "Parser does not yet support STARTS WITH in WHERE clause expressions. \
            The StringOp AST node and executor logic exist, but the parser fails \
            to parse 'n.name STARTS WITH ...' syntax."]
async fn test_string_starts_with() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name STARTS WITH 'Al' RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Alice starts with 'Al'");
    assert_eq!(names[0], "Alice");
}

// ============================================================================
// 6. String CONTAINS
// ============================================================================

#[tokio::test]
#[ignore = "Parser does not yet support CONTAINS in WHERE clause expressions. \
            The StringOp AST node and executor logic exist, but the parser fails \
            to parse 'n.name CONTAINS ...' syntax."]
async fn test_string_contains() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name CONTAINS 'ob' RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Bob contains 'ob'");
    assert_eq!(names[0], "Bob");
}

// ============================================================================
// 7. String ENDS WITH
// ============================================================================

#[tokio::test]
#[ignore = "Parser does not yet support ENDS WITH in WHERE clause expressions. \
            The StringOp AST node and executor logic exist, but the parser fails \
            to parse 'n.name ENDS WITH ...' syntax."]
async fn test_string_ends_with() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name ENDS WITH 'ce' RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Alice ends with 'ce'");
    assert_eq!(names[0], "Alice");
}

// ============================================================================
// 8. IN list predicate
// ============================================================================

#[tokio::test]
async fn test_in_list() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name IN ['Alice', 'Charlie'] RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 2, "Should match Alice and Charlie");
    assert!(names.contains(&"Alice".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
}

// ============================================================================
// 9. CASE expression
// ============================================================================

#[tokio::test]
async fn test_case_expression() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name, CASE WHEN n.age > 30 THEN 'senior' ELSE 'junior' END AS category ORDER BY n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 5);

    // Build a map of name -> category for easy lookup
    let mut categories = std::collections::HashMap::new();
    for row in &result.rows {
        let name: String = row.get("n.name").unwrap();
        let cat: String = row.get("category").unwrap();
        categories.insert(name, cat);
    }

    // Alice(25), Bob(30), Diana(28), Eve(22) are <= 30, so "junior"
    // Charlie(35) is > 30, so "senior"
    assert_eq!(categories.get("Alice").unwrap(), "junior");
    assert_eq!(categories.get("Bob").unwrap(), "junior"); // 30 is not > 30
    assert_eq!(categories.get("Charlie").unwrap(), "senior");
    assert_eq!(categories.get("Diana").unwrap(), "junior");
    assert_eq!(categories.get("Eve").unwrap(), "junior");
}

// ============================================================================
// 10. Type coercion: integer compared to float property
// ============================================================================

#[tokio::test]
async fn test_type_coercion_int_float() {
    let graph = Graph::open_memory().await.unwrap();

    // Create items with float prices
    graph
        .mutate(
            "CREATE (n:Item {name: 'Widget', price: 9.99})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Item {name: 'Gadget', price: 19.99})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Item {name: 'Doohickey', price: 5.50})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Compare float property against integer literal
    let result = graph
        .execute(
            "MATCH (n:Item) WHERE n.price > 10 RETURN n.name ORDER BY n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Gadget (19.99) is > 10");
    assert_eq!(names[0], "Gadget");
}

// ============================================================================
// 11. Arithmetic expression in RETURN
// ============================================================================

#[tokio::test]
async fn test_arithmetic_expression() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Item {name: 'Widget', price: 10, quantity: 5})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Item) RETURN n.price * n.quantity AS total",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let total: i64 = result.rows[0].get("total").unwrap();
    assert_eq!(total, 50, "10 * 5 = 50");
}

// ============================================================================
// 12. String concatenation with +
// ============================================================================

#[tokio::test]
async fn test_string_concatenation() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {first: 'Ada', last: 'Lovelace'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.first + ' ' + n.last AS fullname",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let fullname: String = result.rows[0].get("fullname").unwrap();
    assert_eq!(fullname, "Ada Lovelace");
}

// ============================================================================
// 13. Parameter substitution
// ============================================================================

#[tokio::test]
async fn test_parameter_substitution() {
    let graph = setup_people().await;

    let mut params = PropertyMap::new();
    params.insert("name".into(), Value::from("Alice"));

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.name = $name RETURN n.name, n.age",
            params,
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1, "Should find exactly one person named Alice");
    let name: String = result.rows[0].get("n.name").unwrap();
    let age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(name, "Alice");
    assert_eq!(age, 25);
}

// ============================================================================
// 14. Boolean logic: AND / OR
// ============================================================================

#[tokio::test]
async fn test_boolean_logic_and_or() {
    let graph = setup_people().await;

    // AND: age > 20 AND age < 30 => Alice(25), Diana(28), Eve(22)
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.age > 20 AND n.age < 30 RETURN n.name ORDER BY n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 3, "Alice(25), Diana(28), Eve(22) are between 20 and 30");
    assert!(names.contains(&"Alice".to_string()));
    assert!(names.contains(&"Diana".to_string()));
    assert!(names.contains(&"Eve".to_string()));
}

// ============================================================================
// 15. NOT expression
// ============================================================================

#[tokio::test]
async fn test_not_expression() {
    let graph = Graph::open_memory().await.unwrap();

    graph
        .mutate(
            "CREATE (n:Person {name: 'Alice', active: true})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    graph
        .mutate(
            "CREATE (n:Person {name: 'Bob', active: false})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) WHERE NOT n.active RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Bob has active=false");
    assert_eq!(names[0], "Bob");
}

// ============================================================================
// 16. UNWIND list (parser may not support standalone UNWIND)
// ============================================================================

#[tokio::test]
#[ignore = "UNWIND is recognized by the lexer and appears in LogicalPlan and executor, \
            but the parser does not yet handle UNWIND as a clause within query statements."]
async fn test_unwind_list() {
    let graph = Graph::open_memory().await.unwrap();

    let result = graph
        .execute(
            "UNWIND [1, 2, 3] AS x RETURN x",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 3);
}

// ============================================================================
// 17. count(*) — count all matched nodes
// ============================================================================

#[tokio::test]
async fn test_count_star() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN count(n)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let total: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(total, 5, "Should count all 5 Person nodes");
}

// ============================================================================
// 18. Multiple labels: create node with two labels, match by either
// ============================================================================

#[tokio::test]
async fn test_multiple_labels_match() {
    let graph = Graph::open_memory().await.unwrap();

    // Create a node with two labels: Person AND Employee
    graph
        .mutate(
            "CREATE (n:Person:Employee {name: 'Ada'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();
    // Create a node with only Person label
    graph
        .mutate(
            "CREATE (n:Person {name: 'Bob'})",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Match by Employee label — should only find Ada
    let result = graph
        .execute(
            "MATCH (n:Employee) RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 1, "Only Ada has the Employee label");
    assert_eq!(names[0], "Ada");

    // Match by Person label — should find both
    let result2 = graph
        .execute(
            "MATCH (n:Person) RETURN n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        result2.rows.len(), 2,
        "Both Ada and Bob have the Person label"
    );
}
