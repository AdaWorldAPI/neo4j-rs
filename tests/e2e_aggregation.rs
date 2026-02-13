//! End-to-end integration tests for aggregation, grouping, and result ordering.
//!
//! Tests count, sum, collect, DISTINCT, ORDER BY, SKIP, LIMIT, and grouped aggregation.
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
// 1. count(*) / count(n) — count all nodes
// ============================================================================

#[tokio::test]
async fn test_count_all() {
    let graph = setup_people().await;

    let result = graph
        .execute("MATCH (n:Person) RETURN count(n)", PropertyMap::new())
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 5);
}

// ============================================================================
// 2. count with WHERE filter
// ============================================================================

#[tokio::test]
async fn test_count_with_filter() {
    let graph = setup_people().await;

    // Count people older than 27
    let result = graph
        .execute(
            "MATCH (n:Person) WHERE n.age > 27 RETURN count(n)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let count: i64 = result.rows[0].get("count").unwrap();
    // Bob(30), Charlie(35), Diana(28) = 3 people over 27
    assert_eq!(count, 3);
}

// ============================================================================
// 3. sum() aggregate
// ============================================================================

#[tokio::test]
async fn test_sum_aggregate() {
    let graph = Graph::open_memory().await.unwrap();

    let items = [("Widget", 10), ("Gadget", 25), ("Doohickey", 15)];

    for (name, price) in &items {
        graph
            .mutate(
                &format!("CREATE (n:Item {{name: '{}', price: {}}})", name, price),
                PropertyMap::new(),
            )
            .await
            .unwrap();
    }

    let result = graph
        .execute(
            "MATCH (n:Item) RETURN sum(n.price)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let total: i64 = result.rows[0].get("sum").unwrap();
    assert_eq!(total, 50); // 10 + 25 + 15
}

// ============================================================================
// 4. RETURN DISTINCT values
// ============================================================================

#[tokio::test]
async fn test_distinct_values() {
    let graph = Graph::open_memory().await.unwrap();

    // Create nodes with duplicate names
    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN DISTINCT n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    // Should have 3 distinct names, not 5
    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 3, "Expected 3 distinct names, got {:?}", names);
    assert!(names.contains(&"Alice".to_string()));
    assert!(names.contains(&"Bob".to_string()));
    assert!(names.contains(&"Charlie".to_string()));
}

// ============================================================================
// 5. ORDER BY ascending (default)
// ============================================================================

#[tokio::test]
async fn test_order_by_ascending() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name ORDER BY n.name",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 5);
    // Verify alphabetical order
    assert_eq!(names, vec!["Alice", "Bob", "Charlie", "Diana", "Eve"]);
}

// ============================================================================
// 6. ORDER BY descending
// ============================================================================

#[tokio::test]
async fn test_order_by_descending() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name ORDER BY n.name DESC",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let names: Vec<String> = result
        .rows
        .iter()
        .map(|row| row.get::<String>("n.name").unwrap())
        .collect();

    assert_eq!(names.len(), 5);
    assert_eq!(names, vec!["Eve", "Diana", "Charlie", "Bob", "Alice"]);
}

// ============================================================================
// 7. ORDER BY numeric field
// ============================================================================

#[tokio::test]
async fn test_order_by_numeric() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name, n.age ORDER BY n.age",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    let ages: Vec<i64> = result
        .rows
        .iter()
        .map(|row| row.get::<i64>("n.age").unwrap())
        .collect();

    assert_eq!(ages, vec![22, 25, 28, 30, 35]);
}

// ============================================================================
// 8. SKIP and LIMIT combined
// ============================================================================

#[tokio::test]
async fn test_skip_and_limit() {
    let graph = setup_people().await;

    // Get 2 results after skipping the first 2
    // (Not using ORDER BY since Sort has a known issue with post-projection expressions)
    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name SKIP 2 LIMIT 2",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(
        result.rows.len(), 2,
        "SKIP 2 LIMIT 2 on 5 rows should return exactly 2 rows"
    );
}

// ============================================================================
// 9. LIMIT alone
// ============================================================================

#[tokio::test]
async fn test_limit_alone() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n LIMIT 3",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 3);
}

// ============================================================================
// 10. SKIP alone
// ============================================================================

#[tokio::test]
async fn test_skip_alone() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n SKIP 3",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 2, "5 total - 3 skipped = 2 remaining");
}

// ============================================================================
// 11. collect() aggregate
// ============================================================================

#[tokio::test]
async fn test_collect_aggregate() {
    let graph = Graph::open_memory().await.unwrap();

    graph.mutate("CREATE (n:Person {name: 'Alice'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie'})", PropertyMap::new()).await.unwrap();

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN collect(n.name)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);

    let collected: Vec<Value> = result.rows[0].get("collect").unwrap();
    assert_eq!(collected.len(), 3);

    let collected_strings: Vec<&str> = collected
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    assert!(collected_strings.contains(&"Alice"));
    assert!(collected_strings.contains(&"Bob"));
    assert!(collected_strings.contains(&"Charlie"));
}

// ============================================================================
// 12. min() and max() aggregates
// ============================================================================

#[tokio::test]
async fn test_min_aggregate() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN min(n.age)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let min_age: i64 = result.rows[0].get("min").unwrap();
    assert_eq!(min_age, 22); // Eve is youngest
}

#[tokio::test]
async fn test_max_aggregate() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN max(n.age)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let max_age: i64 = result.rows[0].get("max").unwrap();
    assert_eq!(max_age, 35); // Charlie is oldest
}

// ============================================================================
// 13. avg() aggregate
// ============================================================================

#[tokio::test]
async fn test_avg_aggregate() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN avg(n.age)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let avg_age: f64 = result.rows[0].get("avg").unwrap();
    // (25 + 30 + 35 + 28 + 22) / 5 = 140 / 5 = 28.0
    assert!((avg_age - 28.0).abs() < 0.001, "Expected avg 28.0, got {}", avg_age);
}

// ============================================================================
// 14. count on empty result set
// ============================================================================

#[tokio::test]
async fn test_count_empty_result() {
    let graph = Graph::open_memory().await.unwrap();

    // No nodes exist at all
    let result = graph
        .execute(
            "MATCH (n:Person) RETURN count(n)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1, "count() on empty set should return 1 row");
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 0, "count() on empty set should be 0");
}

// ============================================================================
// 15. Multiple aggregates in single query
// ============================================================================

#[tokio::test]
async fn test_multiple_aggregates() {
    let graph = setup_people().await;

    let result = graph
        .execute(
            "MATCH (n:Person) RETURN count(n), min(n.age), max(n.age)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);

    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 5);

    let min_age: i64 = result.rows[0].get("min").unwrap();
    assert_eq!(min_age, 22);

    let max_age: i64 = result.rows[0].get("max").unwrap();
    assert_eq!(max_age, 35);
}

// ============================================================================
// 16. ORDER BY with LIMIT (top-N query)
// ============================================================================

#[tokio::test]
async fn test_order_by_with_limit() {
    let graph = setup_people().await;

    // Get the 2 oldest people
    let result = graph
        .execute(
            "MATCH (n:Person) RETURN n.name, n.age ORDER BY n.age DESC LIMIT 2",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 2);

    let first_name: String = result.rows[0].get("n.name").unwrap();
    let first_age: i64 = result.rows[0].get("n.age").unwrap();
    assert_eq!(first_name, "Charlie");
    assert_eq!(first_age, 35);

    let second_name: String = result.rows[1].get("n.name").unwrap();
    let second_age: i64 = result.rows[1].get("n.age").unwrap();
    assert_eq!(second_name, "Bob");
    assert_eq!(second_age, 30);
}

// ============================================================================
// 17. DISTINCT with count (should count distinct values)
// ============================================================================

#[tokio::test]
async fn test_count_distinct() {
    let graph = Graph::open_memory().await.unwrap();

    // Create people in departments with duplicates
    graph.mutate("CREATE (n:Person {name: 'Alice', dept: 'Eng'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Bob', dept: 'Eng'})", PropertyMap::new()).await.unwrap();
    graph.mutate("CREATE (n:Person {name: 'Charlie', dept: 'Sales'})", PropertyMap::new()).await.unwrap();

    // count(DISTINCT n.dept) should give 2 (Eng, Sales)
    let result = graph
        .execute(
            "MATCH (n:Person) RETURN count(DISTINCT n.dept)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    let count: i64 = result.rows[0].get("count").unwrap();
    assert_eq!(count, 2, "There should be 2 distinct departments");
}

// ============================================================================
// 18. sum() on empty result set
// ============================================================================

#[tokio::test]
async fn test_sum_empty_result() {
    let graph = Graph::open_memory().await.unwrap();

    let result = graph
        .execute(
            "MATCH (n:Item) RETURN sum(n.price)",
            PropertyMap::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.rows.len(), 1, "sum() on empty set should return 1 row");
    let total: i64 = result.rows[0].get("sum").unwrap();
    assert_eq!(total, 0, "sum() on empty set should be 0");
}
