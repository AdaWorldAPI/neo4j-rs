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

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `bolt` | off | Neo4j Bolt wire protocol client |
| `ladybug` | off | ladybug-rs storage backend (Hamming-accelerated) |
| `ladybug-contract` | off | CogRecord8K types for Hamming queries |
| `arrow-results` | off | Stream results as Arrow RecordBatch |
| `chess` | off | Chess game analysis procedures |
| `full` | off | All features except chess |

## Module Structure

```
src/
├── cypher/          # openCypher parser (lexer → AST → parser) — pure functions, zero I/O
├── planner/         # Logical plan generation (backend-agnostic)
├── execution/       # Plan executor against StorageBackend trait
├── storage/         # Pluggable backends (memory, Bolt, ladybug-rs)
├── model/           # Core DTOs: Node, Relationship, Value, Path, PropertyMap
├── index/           # Graph indexing
├── tx/              # Transaction management
├── export.rs        # Graph import/export
├── aiwar.rs         # AI War Cloud graph procedures
└── chess.rs         # Chess analysis procedures
tests/
├── e2e_basic.rs     # CRUD operations
├── e2e_write.rs     # Write/mutation tests
├── e2e_traversal.rs # Graph traversal patterns
├── e2e_aggregation.rs # COUNT, SUM, AVG, etc.
├── e2e_compound.rs  # Complex multi-clause queries
├── e2e_edge_cases.rs # NULL semantics, empty graphs, etc.
├── e2e_aiwar.rs     # AI War graph integration
├── e2e_ladybug.rs   # Ladybug backend tests
└── e2e_export_roundtrip.rs  # Export/import fidelity
```

## Testing

```bash
cargo test                    # Default (memory backend)
cargo test --all-features     # All backends
cargo test --test e2e_basic   # Specific test suite
```

## Contributing

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full development guide —
prerequisites, building, testing, module walkthrough, and coding conventions.

## Reference

Clean-room reimplementation referencing:
- [AdaWorldAPI/neo4j](https://github.com/AdaWorldAPI/neo4j) — Neo4j 5.26.0 Java source (authoritative reference)
- [openCypher](https://opencypher.org/) — Cypher language specification

## Related Projects

| Crate | Role |
|-------|------|
| [holograph](https://github.com/AdaWorldAPI/holograph) | Bitpacked vector primitives (Hamming, GraphBLAS, HDR cascade) |
| [ladybug-rs](https://github.com/AdaWorldAPI/ladybug-rs) | Hamming-accelerated storage engine (16K fingerprints, LanceDB, DataFusion) |
| [aiwar-neo4j-harvest](https://github.com/AdaWorldAPI/aiwar-neo4j-harvest) | Graph pattern harvester for AI War Cloud dataset |
| [q2](https://github.com/AdaWorldAPI/q2) | Quarto 2 — graph notebook integration |

## License

Apache-2.0
