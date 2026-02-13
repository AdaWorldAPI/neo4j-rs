# CLAUDE.md — neo4j-rs Development Guide

## Project Identity

**neo4j-rs** is a clean-room Rust reimplementation of Neo4j's property graph
database. Zero technical debt by design. Full Cypher. Pluggable storage.

## Reference Repository

The original Neo4j Java source is forked at:
- **https://github.com/AdaWorldAPI/neo4j** (branch: `release/5.26.0`)
- This is the upstream Neo4j 5.26.0 Java codebase
- Use it as the **authoritative reference** for:
  - Cypher grammar and semantics
  - Bolt wire protocol format
  - openCypher TCK (Technology Compatibility Kit) test cases
  - Property graph model behavior (NULL semantics, type coercion, etc.)
  - Transaction isolation semantics

When implementing Cypher features, ALWAYS cross-reference the Java source:
```
neo4j/community/cypher/cypher-planner/    → planner logic
neo4j/community/cypher/front-end/         → parser & AST
neo4j/community/bolt/                     → Bolt protocol
neo4j/community/kernel/                   → storage engine concepts
```

## Sibling Repositories

This crate sits in a trinity of Rust projects:

### holograph (https://github.com/AdaWorldAPI/holograph)
- **Role**: Pure Rust bitpacked vector primitives library
- **Provides**: Hamming SIMD, GraphBLAS sparse matrices, HDR cascade, DN-Tree
- **neo4j-rs uses it**: INDIRECTLY, through ladybug-rs only
- **Key insight**: holograph/src/graphblas/ IS "blasgraph" — the RedisGraph-
  compatible sparse matrix layer. holograph/src/blasgraph/ (to be created)
  will be the user-facing facade for GRAPH.QUERY commands.
- **DO NOT import holograph types into neo4j-rs directly**

### ladybug-rs (https://github.com/AdaWorldAPI/ladybug-rs)
- **Role**: The Hamming-accelerated storage engine
- **Provides**: 16384-bit fingerprint nodes, LanceDB+DataFusion persistence,
  DN-Tree addressing, Cypher→DataFusion SQL transpilation
- **neo4j-rs uses it**: As one `StorageBackend` implementation (feature-gated)
- **Key docs**: ladybug-rs/docs/TECHNICAL_DEBT.md (9 known race conditions),
  ladybug-rs/ARCHITECTURE.md
- **DO NOT import ladybug internal types into neo4j-rs core**

## Architecture Rules

### 1. The StorageBackend Trait Is Sacred

`src/storage/mod.rs` defines `StorageBackend` — the ONLY contract between
neo4j-rs and any storage engine. ALL storage access goes through this trait.

```
neo4j-rs (parser + planner + executor)
    │
    ├── StorageBackend::Memory     (in-process HashMap, for testing)
    ├── StorageBackend::Bolt       (external Neo4j via Bolt protocol)
    └── StorageBackend::Ladybug    (ladybug-rs, feature-gated)
```

### 2. Clean DTO Boundary

The `model/` module defines the ONLY types that cross the storage boundary:
- `Node`, `Relationship`, `Path`, `Value`, `PropertyMap`
- `NodeId`, `RelId`, `Direction`

NEVER let Arrow types, Lance types, holograph types, or any backend-specific
types leak into the core neo4j-rs modules.

### 3. Parser Owns Nothing

The Cypher parser (`cypher/`) produces an AST. It has:
- Zero imports from storage
- Zero imports from execution
- Zero async
- Zero I/O

It is a **pure function**: `&str → Result<Statement>`.

### 4. Planner Is Backend-Agnostic

The planner produces `LogicalPlan` nodes (Scan, Expand, Filter, Project...).
It does NOT know whether the backend is memory, Bolt, or ladybug-rs.
Physical plan adaptation happens in the execution engine.

### 5. The Memory Backend Is The Test Oracle

ALL Cypher behavior must work identically on `MemoryBackend` first.
Then verify the same behavior on Bolt (against real Neo4j from the
reference repo) and ladybug-rs.

## Implementation Priority

```
Phase 1: Cypher Parser (CURRENT)
  ├── Lexer ✓ (functional, tested)
  ├── AST types ✓ (complete openCypher coverage)
  ├── Parser → recursive descent, full MATCH/CREATE/SET/DELETE
  └── Pretty printer (AST → Cypher string, for debugging)

Phase 2: Memory Backend + Execution
  ├── Memory backend ✓ (CRUD + traversal working)
  ├── Execution engine (walk LogicalPlan → StorageBackend calls)
  └── End-to-end: parse → plan → execute → result

Phase 3: Bolt Protocol Client
  ├── PackStream serialization (Neo4j binary format)
  ├── Bolt handshake + authentication
  ├── Run query + stream results
  └── Transaction management (BEGIN/COMMIT/ROLLBACK)

Phase 4: ladybug-rs Integration
  ├── impl StorageBackend for LadybugBackend
  ├── Cypher logical plan → DataFusion physical plan
  ├── Hamming-accelerated MATCH patterns
  └── CALL procedures for vector similarity
```

## Ecosystem References (see docs/INSPIRATION.md for full analysis)

### Bolt Protocol Implementation → steal from neo4rs
- **https://github.com/neo4j-labs/neo4rs** — community Rust driver, 279★
- Their `packstream/` module is gold: serde-based PackStream encode/decode
- Their `bolt/request/` has all message types with binary signatures
- Their `BoltBytesBuilder` test helper is essential for unit testing
- **LIMITATION**: only supports Bolt 4.0-4.3, we need 5.x
- Use their `#[derive(BoltStruct)] #[signature(0xB3, 0x4E)]` pattern

### Official Rust Driver Patterns → borrow architecture
- **https://github.com/robsdedude/neo4j-rust-driver** (publishes `neo4j` crate)
- Same author as the Python Rust extension that got 10x speedup
- **ValueSend/ValueReceive split** — users can't send Nodes as parameters
- **Driver → Session → Transaction** hierarchy with connection pool
- **Causal consistency** via bookmarks (abstract tokens for DB state)
- **Routing** with Read/Write distinction for clusters
- `value_map!` macro for ergonomic parameters

### PackStream Is THE Hot Path
- **https://neo4j.com/blog/developer/python-driver-10x-faster-with-rust/**
- Neo4j rewrote Python's PackStream in Rust → 3-10x speedup
- Binary format: marker byte + length + data (see neo4rs packstream/mod.rs)
- Uses `bytes::Bytes` for zero-copy, serde for type dispatch
- Profile this FIRST when optimizing

### Embedded DB Architecture → borrow executor model from stoolap
- **https://github.com/stoolap/stoolap** — embedded SQL in pure Rust
- Volcano-style pull operators: `fn next() -> Option<Row>`
- Cost-based optimizer with cardinality estimation
- Clean pipeline: parser → planner → optimizer → executor → storage
- MVCC with snapshot isolation
- We adapt this for graph: add Expand, VarLengthExpand, ShortestPath operators

### CLAM/CAKES/panCAKES → academic foundation for fingerprint-based search
- **https://github.com/URI-ABD/clam** — MIT-licensed Rust, 21★
- **CAKES** (arXiv:2309.05491) — exact k-NN scaling with *fractal dimension*, not cardinality
- **panCAKES** (arXiv:2409.12161) — search on compressed data without decompression (70x ratio)
- **Directly validates our Hamming fingerprint approach**: their LFD = our pruning ratio
- CLAM Tree + Bipolar Split = our FingerprintIndex structure
- Their `Search` trait + `KnnBranch` greedy algorithm = our graph traversal via pruning
- SIMD `distances` crate has Hamming, cosine, euclidean — plug directly into our metric
- Theoretical guarantee: O(k · 2^LFD · log(n)), sublinear for real-world data
- Their `DistanceValue` trait with blanket impl is cleaner than hand-rolling

## Coding Standards

- **Edition 2024**, rust-version 1.88
- `thiserror` for error types, NOT `anyhow` in library code
- `async_trait` for async trait methods
- `parking_lot` for synchronization (not std::sync)
- Tests: `#[tokio::test]` for async, regular `#[test]` for sync
- All public types derive `Debug, Clone, Serialize, Deserialize` where possible
- Property maps use `HashMap<String, Value>` (std HashMap, not hashbrown in API)

## Testing Strategy

```
Unit tests:     Per-module, test internal logic
Integration:    End-to-end Cypher → result against MemoryBackend
Compatibility:  openCypher TCK tests (from reference neo4j repo)
Cross-backend:  Same query against Memory, Bolt, Ladybug — results must match
Benchmarks:     criterion benchmarks for parser, execution, traversal
```

## File Layout

```
neo4j-rs/
├── CLAUDE.md              ← YOU ARE HERE
├── Cargo.toml
├── src/
│   ├── lib.rs             ← Public API: Graph<B>, Error, re-exports
│   ├── model/             ← DTOs (Node, Relationship, Value, Path)
│   ├── cypher/            ← Parser (lexer → AST), pure functions
│   ├── planner/           ← Logical plan, optimizer
│   ├── execution/         ← Execute plan against StorageBackend
│   ├── storage/           ← StorageBackend trait + implementations
│   │   ├── mod.rs         ← THE TRAIT
│   │   ├── memory.rs      ← Reference in-memory implementation
│   │   ├── bolt.rs        ← Neo4j Bolt protocol (feature: bolt)
│   │   └── ladybug.rs     ← ladybug-rs backend (feature: ladybug)
│   ├── tx/                ← Transaction types
│   └── index/             ← Index types
├── tests/
└── benches/
```

## Quick Commands

```bash
# Run all tests
cargo test

# Run with all features
cargo test --all-features

# Run specific test
cargo test test_create_and_get_node

# Check (fast compile check)
cargo check

# Benchmark parser
cargo bench --bench cypher_bench
```

## What NOT To Do

- Do NOT add `holograph` as a direct dependency
- Do NOT add `arrow` or `datafusion` to the default feature set
- Do NOT make the parser async
- Do NOT store backend-specific data in model types
- Do NOT skip the MemoryBackend tests ("it works on Bolt" is not enough)
- Do NOT implement APOC procedures (that's a future extension crate)
- Do NOT add Redis, HTTP, or gRPC server code (that's a separate binary crate)
