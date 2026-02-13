# neo4j-rs

Clean Rust reimplementation of Neo4j — property graph database with full Cypher support.

## Why

Neo4j is brilliant but carries decades of JVM technical debt. This is the clean-room rewrite:

- **Full Cypher** — openCypher-compliant parser, planner, and executor
- **Pluggable storage** — `StorageBackend` trait with memory, Bolt, and [ladybug-rs](https://github.com/AdaWorldAPI/ladybug-rs) backends
- **Zero technical debt** — trait-first design, clean DTO boundaries, no legacy
- **Bolt compatible** — connect to existing Neo4j clusters via wire protocol
- **Hamming-accelerated** — via ladybug-rs + [holograph](https://github.com/AdaWorldAPI/holograph) for vector-native graph traversal

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│                      neo4j-rs                             │
│  Cypher Parser → Planner → Executor                      │
│                     │                                     │
│          ┌──────────┼──────────┐                         │
│          │          │          │                          │
│     MemoryBackend  Bolt    LadybugBackend                │
│     (testing)    (Neo4j)  (Hamming-accelerated)          │
│                              │                            │
│                         ladybug-rs                        │
│                              │                            │
│                          holograph                        │
│                    (bitpacked primitives)                 │
└──────────────────────────────────────────────────────────┘
```

## Quick Start

```rust
use neo4j_rs::{Graph, Value, Node};

#[tokio::main]
async fn main() -> neo4j_rs::Result<()> {
    let graph = Graph::open_memory().await?;

    graph.mutate(
        "CREATE (n:Person {name: $name, age: $age}) RETURN n",
        [("name", Value::from("Ada")), ("age", Value::from(3))],
    ).await?;

    let result = graph.execute(
        "MATCH (n:Person) WHERE n.name = $name RETURN n",
        [("name", Value::from("Ada"))],
    ).await?;

    for row in result.rows {
        let node: Node = row.get("n")?;
        println!("{:?}", node);
    }

    Ok(())
}
```

## Contributing

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full development guide —
prerequisites, building, testing, module walkthrough, and coding conventions.

## Reference

This is a clean-room reimplementation referencing:
- [AdaWorldAPI/neo4j](https://github.com/AdaWorldAPI/neo4j) — Neo4j 5.26.0 Java source (authoritative reference)
- [openCypher](https://opencypher.org/) — Cypher language specification

## Related Projects

| Crate | Role |
|-------|------|
| [holograph](https://github.com/AdaWorldAPI/holograph) | Bitpacked vector primitives (Hamming, GraphBLAS, HDR cascade) |
| [ladybug-rs](https://github.com/AdaWorldAPI/ladybug-rs) | Hamming-accelerated storage engine (16K fingerprints, LanceDB, DataFusion) |
| **neo4j-rs** | Property graph interface (Cypher, transactions, pluggable storage) |

## License

Apache-2.0
