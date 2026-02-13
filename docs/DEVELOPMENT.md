# Development Guide — neo4j-rs

> The single comprehensive reference for contributors. Merges project identity,
> architecture rules, ecosystem analysis, and practical how-to into one document.
>
> See also: [ARCHITECTURE.md](../ARCHITECTURE.md) (system design),
> [INSPIRATION.md](INSPIRATION.md) (ecosystem deep-dive).

---

## Table of Contents

1. [Project Identity & Reference Repo](#1-project-identity--reference-repo)
2. [Prerequisites & Sibling Repositories](#2-prerequisites--sibling-repositories)
3. [Building](#3-building)
4. [Testing](#4-testing)
5. [Project Status & Implementation Phases](#5-project-status--implementation-phases)
6. [Architecture Rules](#6-architecture-rules)
7. [Module Walkthrough & File Layout](#7-module-walkthrough--file-layout)
8. [Data Flow](#8-data-flow)
9. [Adding Code: Patterns to Follow](#9-adding-code-patterns-to-follow)
10. [Error Handling](#10-error-handling)
11. [Feature Flags](#11-feature-flags)
12. [Coding Conventions](#12-coding-conventions)
13. [Ecosystem: What to Steal & Borrow](#13-ecosystem-what-to-steal--borrow)
14. [Integration Contract: neo4j-rs ↔ ladybug-rs](#14-integration-contract-neo4j-rs--ladybug-rs)
15. [What to Work On](#15-what-to-work-on)
16. [Reference Resources](#16-reference-resources)
17. [What NOT to Do](#17-what-not-to-do)

---

## 1. Project Identity & Reference Repo

**neo4j-rs** is a clean-room Rust reimplementation of Neo4j's property graph
database. Zero technical debt by design. Full Cypher. Pluggable storage.

### Reference Repository

The original Neo4j Java source is forked at:

- **https://github.com/AdaWorldAPI/neo4j** (branch: `release/5.26.0`)
- This is the upstream Neo4j 5.26.0 Java codebase
- Use it as the **authoritative reference** for:
  - Cypher grammar and semantics
  - Bolt wire protocol format
  - openCypher TCK (Technology Compatibility Kit) test cases
  - Property graph model behavior (NULL semantics, type coercion, etc.)
  - Transaction isolation semantics

When implementing Cypher features, **ALWAYS** cross-reference the Java source:

```
neo4j/community/cypher/cypher-planner/    → planner logic
neo4j/community/cypher/front-end/         → parser & AST
neo4j/community/bolt/                     → Bolt protocol
neo4j/community/kernel/                   → storage engine concepts
```

---

## 2. Prerequisites & Sibling Repositories

### Toolchain

| Requirement | Version | Notes |
|------------|---------|-------|
| **Rust** | 1.88+ | Edition 2024. Install via [rustup](https://rustup.rs/) |
| **cargo** | (bundled) | Comes with Rust |
| **Git** | any | For version control |
| **Docker** | optional | For Bolt integration tests against real Neo4j |

No C dependencies. No system libraries. Pure Rust by design.

```bash
# Verify your toolchain
rustc --version   # must be >= 1.88
cargo --version
```

### The Three-Crate Trinity

This crate sits in a trinity of Rust projects:

```
parent/
├── neo4j-rs/          # THIS REPO — Cypher parser + planner + executor
├── ladybug-rs/        # Hamming-accelerated storage engine
└── holograph/         # Bitpacked vector primitives (used by ladybug-rs)
```

#### holograph ([github.com/AdaWorldAPI/holograph](https://github.com/AdaWorldAPI/holograph))

- **Role**: Pure Rust bitpacked vector primitives library
- **Provides**: Hamming SIMD, GraphBLAS sparse matrices, HDR cascade, DN-Tree
- **neo4j-rs uses it**: INDIRECTLY, through ladybug-rs only
- **Key insight**: `holograph/src/graphblas/` IS "blasgraph" — the RedisGraph-
  compatible sparse matrix layer. `holograph/src/blasgraph/` (to be created)
  will be the user-facing facade for `GRAPH.QUERY` commands.
- **DO NOT import holograph types into neo4j-rs directly**

#### ladybug-rs ([github.com/AdaWorldAPI/ladybug-rs](https://github.com/AdaWorldAPI/ladybug-rs))

- **Role**: The Hamming-accelerated storage engine
- **Provides**: 16384-bit fingerprint nodes, LanceDB+DataFusion persistence,
  DN-Tree addressing, Cypher→DataFusion SQL transpilation
- **neo4j-rs uses it**: As one `StorageBackend` implementation (feature-gated)
- **Key docs**: `ladybug-rs/docs/TECHNICAL_DEBT.md` (9 known race conditions),
  `ladybug-rs/ARCHITECTURE.md`
- **DO NOT import ladybug internal types into neo4j-rs core**

If you need the `ladybug` feature, clone the sibling repo:

```bash
cd ..
git clone https://github.com/AdaWorldAPI/ladybug-rs
```

The `ladybug` dependency uses `path = "../ladybug-rs"` in `Cargo.toml`.

---

## 3. Building

```bash
# Fast compile check (no codegen, catches all errors)
cargo check

# Full build (default features only — no bolt, no ladybug, no arrow)
cargo build

# Build with Bolt protocol support
cargo build --features bolt

# Build everything
cargo build --features full

# Benchmark parser
cargo bench --bench cypher_bench
```

### Release Build

The release profile is configured for maximum performance:

```toml
[profile.release]
lto = "fat"           # Full link-time optimization
codegen-units = 1     # Single codegen unit
opt-level = 3         # Maximum optimization
panic = "abort"       # Abort on panic (smaller binary)
```

```bash
cargo build --release
```

---

## 4. Testing

### Quick Commands

```bash
# Run all tests (default features)
cargo test

# Run all tests with all features enabled
cargo test --all-features

# Run a specific test by name
cargo test test_create_and_get_node

# Run tests in a specific module
cargo test storage::memory::tests

# Run tests with output visible
cargo test -- --nocapture

# Run only lexer tests
cargo test cypher::lexer::tests
```

### Testing Strategy (Five Tiers)

```
Unit tests:      Per-module, test internal logic
Integration:     End-to-end Cypher → result against MemoryBackend
Compatibility:   openCypher TCK tests (from reference neo4j repo)
Cross-backend:   Same query against Memory, Bolt, Ladybug — results must match
Benchmarks:      criterion benchmarks for parser, execution, traversal
```

### Test Locations

1. **Inline tests** — `#[cfg(test)] mod tests` at the bottom of source files.
   The lexer (`src/cypher/lexer.rs`), value types (`src/model/value.rs`), and
   memory backend (`src/storage/memory.rs`) already have these.

2. **Integration tests** — `tests/` directory (to be created). These test
   end-to-end flows: parse Cypher, plan, execute against `MemoryBackend`.

### Writing a New Test

For the **memory backend** (async):

```rust
#[tokio::test]
async fn test_my_feature() {
    let db = MemoryBackend::new();
    let mut tx = db.begin_tx(TxMode::ReadWrite).await.unwrap();

    let mut props = PropertyMap::new();
    props.insert("name".into(), Value::from("Ada"));

    let id = db.create_node(&mut tx, &["Person"], props).await.unwrap();
    let node = db.get_node(&tx, id).await.unwrap().unwrap();

    assert_eq!(node.get("name"), Some(&Value::from("Ada")));
}
```

For the **parser** (sync, no I/O):

```rust
#[test]
fn test_parse_return_star() {
    let tokens = tokenize("MATCH (n) RETURN *").unwrap();
    // assert on token kinds...
}
```

### Integration Test Patterns to Borrow (from neo4rs)

neo4rs has excellent integration tests in `integrationtests/tests/` — borrow
their structure:

```
bookmarks.rs              — Causal consistency
dates.rs                  — Date/DateTime round-trip
datetime_as_param.rs      — Temporal types as parameters
duration_deserialization.rs — Duration parsing
nodes.rs                  — Node creation/retrieval
path.rs                   — Path traversal
points.rs                 — Spatial types (Point2D/3D)
relationships.rs          — Relationship CRUD
result_stream.rs          — Streaming large results
result_summary.rs         — Execution statistics
rollback_a_transaction.rs — Transaction rollback semantics
streams_within_a_transaction.rs — Multiple streams in one tx
transactions.rs           — Transaction lifecycle
```

What to borrow:
- The test structure: each concern isolated in its own file
- Container-based testing (they use testcontainers with Neo4j Docker)
- Round-trip testing pattern: create → query → verify for every type
- Edge case coverage: unbounded relationships, missing properties, etc.

### Dev Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` (full features) | Async test runtime |
| `pretty_assertions` | Colored diff on test failures |
| `proptest` | Property-based / fuzz testing |

---

## 5. Project Status & Implementation Phases

### What's Done

| Component | File(s) | Status |
|-----------|---------|--------|
| Public API (`Graph<B>`) | `src/lib.rs` | Complete |
| Error types | `src/lib.rs` | Complete |
| Node, Relationship, Path DTOs | `src/model/` | Complete |
| Value enum (17 variants) | `src/model/value.rs` | Complete |
| PropertyMap | `src/model/property_map.rs` | Complete |
| Cypher lexer | `src/cypher/lexer.rs` | Complete, tested |
| Cypher AST types | `src/cypher/ast.rs` | Complete |
| StorageBackend trait | `src/storage/mod.rs` | Complete |
| MemoryBackend | `src/storage/memory.rs` | Complete, tested |
| Transaction types | `src/tx/mod.rs` | Complete |
| Index types | `src/index/mod.rs` | Complete |

### What's Skeleton / TODO

| Component | File(s) | Status |
|-----------|---------|--------|
| Cypher parser | `src/cypher/parser.rs` | Skeleton — returns error |
| Logical planner | `src/planner/mod.rs` | Types defined, `plan()` returns error |
| Cost optimizer | `src/planner/mod.rs` | `optimize()` is a no-op passthrough |
| Execution engine | `src/execution/mod.rs` | Types defined, `execute()` returns error |
| Bolt backend | `src/storage/bolt.rs` | Not created (feature-gated) |
| Ladybug backend | `src/storage/ladybug.rs` | Not created (feature-gated) |
| Integration tests | `tests/` | Directory does not exist yet |
| Benchmarks | `benches/` | Directory does not exist yet |

### Implementation Phases

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

---

## 6. Architecture Rules

These five rules are **sacred**. Every contributor must understand and follow them.

### Rule 1: The StorageBackend Trait Is Sacred

`src/storage/mod.rs` defines `StorageBackend` — the ONLY contract between
neo4j-rs and any storage engine. ALL storage access goes through this trait.

```
neo4j-rs (parser + planner + executor)
    │
    ├── StorageBackend::Memory     (in-process HashMap, for testing)
    ├── StorageBackend::Bolt       (external Neo4j via Bolt protocol)
    └── StorageBackend::Ladybug    (ladybug-rs, feature-gated)
```

### Rule 2: Clean DTO Boundary

The `model/` module defines the ONLY types that cross the storage boundary:
- `Node`, `Relationship`, `Path`, `Value`, `PropertyMap`
- `NodeId`, `RelId`, `Direction`

**NEVER** let Arrow types, Lance types, holograph types, or any backend-specific
types leak into the core neo4j-rs modules.

### Rule 3: Parser Owns Nothing

The Cypher parser (`cypher/`) produces an AST. It has:
- Zero imports from storage
- Zero imports from execution
- Zero async
- Zero I/O

It is a **pure function**: `&str → Result<Statement>`.

### Rule 4: Planner Is Backend-Agnostic

The planner produces `LogicalPlan` nodes (Scan, Expand, Filter, Project...).
It does NOT know whether the backend is memory, Bolt, or ladybug-rs.
Physical plan adaptation happens in the execution engine.

### Rule 5: The Memory Backend Is The Test Oracle

ALL Cypher behavior must work identically on `MemoryBackend` first.
Then verify the same behavior on Bolt (against real Neo4j from the
reference repo) and ladybug-rs.

### Key Architectural Decisions

These decisions are final. They come from analyzing neo4rs, the official
neo4j driver, Stoolap, and the Python Rust extension blog post.

**From neo4rs + official driver:**

1. Use `bytes::Bytes` for zero-copy PackStream — not `Vec<u8>`
2. Serde for PackStream — don't hand-roll serialization
3. Connection pooling via `deadpool` (or `bb8`) — not hand-rolled
4. TLS via `rustls` — not openssl (no C dependency)
5. `tokio::BufStream` for connection I/O — not raw `TcpStream`

**From Stoolap:**

6. Volcano-style pull operators for the execution engine
7. Cost-based optimizer with cardinality estimation
8. Clean pipeline: parser → planner → optimizer → executor → storage

**From the Python Rust ext blog:**

9. PackStream is THE hot path — optimize this above everything else
10. Profile before optimizing — their 10x came from a very specific bottleneck

**Our unique additions:**

11. `StorageBackend` trait — neither driver nor embedded DB has this
12. Hamming-accelerated traversal — unique to ladybug-rs
13. Fingerprint-based pattern matching — prune 90% before touching properties
14. Graph-specific operators — Expand, VarLengthExpand, ShortestPath

---

## 7. Module Walkthrough & File Layout

### File Layout

```
neo4j-rs/
├── CLAUDE.md              ← AI development instructions
├── ARCHITECTURE.md        ← System design document
├── Cargo.toml
├── docs/
│   ├── DEVELOPMENT.md     ← YOU ARE HERE
│   └── INSPIRATION.md     ← Ecosystem analysis
├── src/
│   ├── lib.rs             ← Public API: Graph<B>, Error, re-exports
│   ├── model/             ← DTOs (Node, Relationship, Value, Path)
│   ├── cypher/            ← Parser (lexer → AST), pure functions
│   │   ├── mod.rs         ← Public parse() function
│   │   ├── lexer.rs       ← Tokenizer
│   │   ├── parser.rs      ← AST construction (stub)
│   │   └── ast.rs         ← Cypher AST types
│   ├── planner/           ← Logical plan, optimizer
│   │   └── mod.rs         ← LogicalPlan enum + plan()/optimize()
│   ├── execution/         ← Execute plan against StorageBackend
│   │   └── mod.rs         ← QueryResult, ResultRow, execute()
│   ├── storage/           ← StorageBackend trait + implementations
│   │   ├── mod.rs         ← THE TRAIT
│   │   ├── memory.rs      ← Reference in-memory implementation
│   │   ├── bolt.rs        ← Neo4j Bolt protocol (feature: bolt)
│   │   └── ladybug.rs     ← ladybug-rs backend (feature: ladybug)
│   ├── tx/                ← Transaction types
│   │   └── mod.rs         ← TxMode, TxId, Transaction trait
│   └── index/             ← Index types
│       └── mod.rs         ← IndexType enum
├── tests/                 ← Integration tests (to be created)
└── benches/               ← Benchmarks (to be created)
```

### `src/lib.rs` — Public API

The `Graph<B: StorageBackend>` struct is the user-facing entry point. It owns
a backend and exposes `execute()` (read) and `mutate()` (write). The full
pipeline is:

```
parse(cypher) → plan(ast) → optimize(plan) → execute(backend, tx, plan)
```

`ExplicitTx` wraps a `Graph` reference + transaction for manual transaction
control via `begin()` / `commit()` / `rollback()`.

### `src/model/` — Data Transfer Objects

Pure data types. No behavior beyond `From` conversions and comparison.

- **`Node`** — `{ id: NodeId, labels: Vec<String>, properties: PropertyMap }`
- **`Relationship`** — `{ id: RelId, src: NodeId, dst: NodeId, rel_type: String, properties: PropertyMap }`
- **`Path`** — Sequence of alternating nodes and relationships
- **`Value`** — 17-variant enum covering Neo4j's full type system (Null, Bool, Int, Float, String, Bytes, List, Map, Node, Relationship, Path, Date, Time, DateTime, LocalDateTime, Duration, Point2D, Point3D)
- **`PropertyMap`** — `HashMap<String, Value>`
- **`NodeId` / `RelId`** — Newtype wrappers around `u64`
- **`Direction`** — `Outgoing | Incoming | Both`

Every public type derives `Debug`, `Clone`, `Serialize`, `Deserialize`.

### `src/cypher/` — Parser (Pure Functions)

The parser module has zero imports from storage or execution. It is
synchronous with no I/O. (See [Architecture Rule 3](#rule-3-parser-owns-nothing).)

**Lexer** (`lexer.rs`): `fn tokenize(&str) -> Result<Vec<Token>>`. Handles
keywords (case-insensitive), string literals with escapes, numbers, parameters
(`$name`), and all Cypher punctuation including arrows (`->`, `<-`), range
(`..`), and compound operators (`<=`, `<>`, `=~`, `+=`).

**AST** (`ast.rs`): Complete type definitions for all Cypher statement types —
`Query`, `Create`, `Merge`, `Delete`, `Set`, `Schema`. Expressions cover
literals, variables, property access, function calls, binary/unary ops, CASE,
EXISTS, IS NULL, label checks, string operations, and wildcards.

**Parser** (`parser.rs`): Currently a stub. Needs a recursive descent parser
that consumes `Vec<Token>` and produces `Statement`. See
[What to Work On](#15-what-to-work-on).

### `src/planner/` — Logical Plan

`LogicalPlan` is an enum of 11 operators: `NodeScan`, `IndexLookup`, `Expand`,
`Filter`, `Project`, `CreateNode`, `CreateRel`, `Limit`, `Sort`,
`CartesianProduct`, `Argument`.

The `plan()` function transforms an AST `Statement` + parameters into a
`LogicalPlan` tree. The `optimize()` function applies rewrite rules (predicate
pushdown, index selection, join ordering).

Both are stubs that need implementation.

### `src/execution/` — Query Execution

`QueryResult` holds columns, rows, and execution statistics. `ResultRow` is a
`HashMap<String, Value>` with typed access via `row.get::<T>(key)`.

The `execute()` function walks a `LogicalPlan` tree and makes
`StorageBackend` calls to produce a `QueryResult`. Currently a stub.

### `src/storage/` — Backend Trait + Implementations

`StorageBackend` is an async trait with ~20 methods grouped into:
- **Lifecycle**: `shutdown()`
- **Transactions**: `begin_tx()`, `commit_tx()`, `rollback_tx()`
- **Node CRUD**: `create_node()`, `get_node()`, `delete_node()`, `set_node_property()`, etc.
- **Relationship CRUD**: `create_relationship()`, `get_relationship()`, `delete_relationship()`
- **Traversal**: `get_relationships()`, `expand()`
- **Index**: `create_index()`, `drop_index()`
- **Schema**: `node_count()`, `relationship_count()`, `labels()`, `relationship_types()`
- **Scan**: `nodes_by_label()`, `nodes_by_property()`

`MemoryBackend` is the reference implementation using `HashMap` +
`parking_lot::RwLock`. It has full CRUD, adjacency tracking, label indexing,
BFS expansion with cycle detection, and basic tests.

### `src/tx/` — Transactions

`TxMode` (ReadOnly / ReadWrite), `TxId` (opaque u64), and the `Transaction`
trait. Currently, `MemoryBackend` transactions are markers without real MVCC.

### `src/index/` — Index Types

`IndexType` enum: `BTree`, `FullText`, `Unique`, `Vector`. Used by
`StorageBackend::create_index()`.

---

## 8. Data Flow

### Read Query: `MATCH (n:Person) WHERE n.age > 25 RETURN n.name`

```
         User
          │
          ▼
  Graph::execute(cypher, params)
          │
          ▼
  ┌── cypher::parse(cypher) ──────────────────────────┐
  │   lexer::tokenize()  →  parser::parse()  →  AST   │
  │   (sync, pure, no I/O)                             │
  └────────────────────────────────────────────────────┘
          │
          ▼
  ┌── planner::plan(ast, params) ─────────────────────┐
  │   AST → LogicalPlan tree                           │
  │   NodeScan("Person") → Filter(age > 25)            │
  │                         → Project(n.name)          │
  └────────────────────────────────────────────────────┘
          │
          ▼
  ┌── planner::optimize(plan) ────────────────────────┐
  │   Rewrite rules: push Filter below Scan,           │
  │   pick IndexLookup if index exists                 │
  └────────────────────────────────────────────────────┘
          │
          ▼
  ┌── execution::execute(backend, tx, plan) ──────────┐
  │   Walk plan tree → call StorageBackend methods     │
  │   Assemble QueryResult rows                        │
  └────────────────────────────────────────────────────┘
          │
          ▼
      QueryResult { columns, rows, stats }
```

### Write Query: `CREATE (n:Person {name: 'Ada'}) RETURN n`

Same pipeline, but `Graph::mutate()` uses `TxMode::ReadWrite` and the
execution engine calls `backend.create_node()`.

---

## 9. Adding Code: Patterns to Follow

### Cross-Reference the Java Source

When implementing any Cypher feature, **ALWAYS** check the Neo4j Java source first:

```
neo4j/community/cypher/cypher-planner/    → planner logic
neo4j/community/cypher/front-end/         → parser & AST
neo4j/community/bolt/                     → Bolt protocol
neo4j/community/kernel/                   → storage engine concepts
```

Reference repo: [github.com/AdaWorldAPI/neo4j](https://github.com/AdaWorldAPI/neo4j)
(branch `release/5.26.0`).

### Adding a New StorageBackend Method

1. Add the method signature to `StorageBackend` in `src/storage/mod.rs`
2. Implement it in `MemoryBackend` (`src/storage/memory.rs`) **first**
3. Write a test in `memory.rs` `mod tests`
4. Only then implement in Bolt/Ladybug backends

### Adding a New AST Node

1. Add the variant to the appropriate enum in `src/cypher/ast.rs`
2. Add lexer support for any new tokens in `src/cypher/lexer.rs`
3. Add parsing logic in `src/cypher/parser.rs`
4. Add planning support in `src/planner/mod.rs`
5. Add execution support in `src/execution/mod.rs`

### Adding a New Value Type

1. Add the variant to `Value` in `src/model/value.rs`
2. Add `type_name()` match arm
3. Add `Display` formatting
4. Add `neo4j_cmp()` comparison logic
5. Add `From` conversion if applicable
6. Add `FromValue` impl in `src/execution/mod.rs`

### Implementing a Parser Rule

The parser should be recursive descent. Follow this pattern:

```rust
fn parse_match_clause(tokens: &[Token], pos: &mut usize) -> Result<MatchClause> {
    expect(tokens, pos, TokenKind::Match)?;
    let patterns = parse_pattern_list(tokens, pos)?;
    let where_clause = if peek(tokens, *pos) == Some(TokenKind::Where) {
        *pos += 1;
        Some(parse_expression(tokens, pos)?)
    } else {
        None
    };
    Ok(MatchClause { optional: false, patterns })
}
```

Key rules:
- The parser takes `&[Token]` and a mutable position index
- Each `parse_*` function advances `pos` past what it consumed
- Return `Err(Error::SyntaxError { position, message })` on failure
- No allocation unless building an AST node
- No async, no I/O, no imports from storage/execution

---

## 10. Error Handling

All errors use the `Error` enum in `src/lib.rs`:

```rust
pub enum Error {
    SyntaxError { position: usize, message: String },  // Parser errors
    SemanticError(String),                               // Type/scope errors
    TypeError { expected: String, got: String },         // Value type mismatches
    PlanError(String),                                   // Planner failures
    ExecutionError(String),                              // Runtime errors
    StorageError(String),                                // Backend errors
    TxError(String),                                     // Transaction errors
    NotFound(String),                                    // Missing entities
    ConstraintViolation(String),                         // Schema violations
    Io(std::io::Error),                                  // I/O errors
}
```

Rules:
- Use `thiserror` derive macros. Never use `anyhow` in library code.
- `SyntaxError` includes the byte position so the caller can show context.
- Functions return `crate::Result<T>` (alias for `std::result::Result<T, Error>`).
- Don't panic. Return `Err(...)` for recoverable failures.
- `ConstraintViolation` is for Neo4j-compatible constraint semantics (e.g., can't
  delete a node that still has relationships).

---

## 11. Feature Flags

```toml
[features]
default = []                                    # No optional features
bolt = ["dep:tokio", "dep:bytes"]              # Bolt protocol client
ladybug = ["dep:ladybug", "dep:tokio"]         # ladybug-rs backend
arrow-results = ["dep:arrow"]                   # Arrow RecordBatch results
full = ["bolt", "ladybug", "arrow-results"]    # Everything
```

The default build has **zero optional dependencies**. Core functionality (parser,
planner, executor, MemoryBackend) works without any features enabled.

### Guidelines

- **Never** add `arrow`, `datafusion`, `tokio`, or `ladybug` to default features
- Feature-gated code uses `#[cfg(feature = "...")]` at the module level
- All behavior must work on `MemoryBackend` first (no features needed)
- The `bolt` feature adds `tokio` (async runtime) and `bytes` (zero-copy buffers)
- The `ladybug` feature expects `../ladybug-rs` to exist on disk

---

## 12. Coding Conventions

### Rust Edition & Toolchain

- **Edition 2024**, `rust-version = "1.88"`
- Format with `cargo fmt` before committing
- Lint with `cargo clippy` — treat warnings as errors

### Dependencies

| Use | Don't use |
|-----|-----------|
| `thiserror` | `anyhow` (library code) |
| `async_trait` | manual vtable hacks |
| `parking_lot::RwLock` | `std::sync::RwLock` |
| `HashMap<String, Value>` (std) | `hashbrown` in public API |
| `#[tokio::test]` | `#[async_std::test]` |

### Type Design

- All public types derive `Debug, Clone` at minimum
- Add `Serialize, Deserialize` where the type may cross a serialization boundary
- Use newtype wrappers for IDs: `NodeId(u64)`, `RelId(u64)`, `TxId(u64)`
- Property maps use `HashMap<String, Value>` (std, not hashbrown in API surface)

### Module Boundaries

- `cypher/` imports from `crate::Error` only. Zero imports from `storage/`, `execution/`, `planner/`.
- `planner/` imports from `cypher/ast` and `model/`. Zero imports from `storage/` or `execution/`.
- `execution/` imports from `planner/`, `model/`, and `storage/` (the trait, not implementations).
- `storage/` imports from `model/`, `tx/`, `index/`. Never imports from `cypher/` or `planner/`.

```
cypher/  ──────→  (standalone, pure)
planner/ ──────→  cypher/ast, model/
execution/ ────→  planner/, model/, storage/ (trait)
storage/ ──────→  model/, tx/, index/
```

### Documentation

- All public items get a `///` doc comment
- Module-level `//!` docs explain the module's role and constraints
- Don't document private helpers unless the logic is non-obvious
- Reference Neo4j semantics when behavior matches (e.g., NULL comparison rules)

---

## 13. Ecosystem: What to Steal & Borrow

### Sources Analyzed

| Source | What It Is | Key Insight |
|--------|-----------|-------------|
| **neo4rs** (neo4j-labs) | Community Rust Bolt driver, 279★ | PackStream serde, BoltStruct macros, integration test patterns |
| **neo4j** (robsdedude/docs.rs) | Official-adjacent Rust driver, 15★ | ValueSend/ValueReceive split, routing, bookmark management, session lifecycle |
| **Python Rust ext** (neo4j blog) | PackStream rewrite in Rust for 10x perf | **THE hot path**: serde-based PackStream is where ALL performance lives |
| **Stoolap** | Embedded SQL db in pure Rust, 0.2 | Volcano executor, cost-based optimizer, MVCC, full architecture reference |

### STEAL: PackStream Serde (from neo4rs)

The `packstream/` module in neo4rs is the single most valuable piece of code.
Neo4j's blog confirmed this: when they rewrote Python's PackStream in Rust,
they got 3-10x speedup. This is THE hot path.

**What neo4rs does brilliantly:**

```rust
// Uses serde's Serializer/Deserializer traits for PackStream encoding
// This means any type that derives Serialize/Deserialize can go over Bolt
pub fn from_bytes<T: DeserializeOwned>(bytes: Bytes) -> Result<T, Error>
pub fn to_bytes<T: Serialize>(value: &T) -> Result<Bytes, Error>

// BoltBytesBuilder for test fixtures — builder pattern for raw bolt bytes
bolt().structure(1, 0x01)
    .tiny_map(4)
    .tiny_string("scheme").tiny_string("basic")
    .build()
```

**What to borrow:**
- The entire PackStream binary format: marker bytes (`0x80`=tiny_string,
  `0x90`=tiny_list, `0xA0`=tiny_map, `0xB0`=struct, `0xC0`=null, etc.)
- The serde-based approach: `impl Serializer for PackStreamSerializer`,
  `impl Deserializer for PackStreamDeserializer`
- The `BoltBytesBuilder` test helper — invaluable for unit testing Bolt messages
- The `#[derive(BoltStruct)] #[signature(0xB3, 0x4E)]` proc macro for Node/Relationship

**What to improve:**
- neo4rs has two separate type hierarchies (`BoltNode` vs `Row::Node`) — we unify at DTO boundary
- Their deserialization does `unsafe { Box::from_raw() }` for zero-copy — profile first, add later
- Support Bolt 5.x (neo4rs only supports 4.0-4.3 currently!)

### STEAL: ValueSend/ValueReceive Split (from official neo4j crate)

The `neo4j` crate on docs.rs (by robsdedude, the same person who built the
Python Rust ext!) has an elegant type split:

```rust
// Values the user SENDS to the database
pub enum ValueSend { Integer(i64), Float(f64), String(String), ... }

// Values the user RECEIVES from the database
pub enum ValueReceive { Integer(i64), Float(f64), String(String), Node(...), Relationship(...), ... }
```

**Why this is smart:**
- Users can't accidentally send a Node as a parameter (Nodes are receive-only)
- The send types can be simpler (no element IDs, no internal metadata)
- Compile-time prevention of common driver misuse
- `value_map!` macro for ergonomic parameter construction

**What to borrow:**
- The ValueSend/ValueReceive distinction (adapt to our `Value`/`ValueOut` or similar)
- The `value_map!` macro pattern
- Their routing + bookmark management patterns (causal consistency across sessions)

### STEAL: Bolt Protocol Messages (from neo4rs)

neo4rs has clean, well-tested message implementations:

```
bolt/request/
├── hello.rs     — HELLO (0x01) with auth + routing context
├── begin.rs     — BEGIN (0x11) with database + bookmarks
├── commit.rs    — COMMIT (0x12)
├── rollback.rs  — ROLLBACK (0x13)
├── pull.rs      — PULL (0x3F) with streaming pagination
├── discard.rs   — DISCARD (0x2F)
├── reset.rs     — RESET (0x0F) for error recovery
├── goodbye.rs   — GOODBYE (0x02)
└── route.rs     — ROUTE (0x66) for server-side routing
```

Each message uses serde for serialization with structure tags. The handshake
negotiation, chunking (`MAX_CHUNK_SIZE = 65535 - 2`), and TLS setup are all
working and tested.

**What to borrow:**
- The complete message catalog with their binary signatures
- Chunked transfer encoding (`u16` length prefix per chunk, `0x0000` terminator)
- Connection pool management (deadpool-based in neo4rs)
- Version negotiation handshake (4-byte magic + supported version ranges)

### BORROW: Volcano-Style Executor (from Stoolap)

Stoolap is the best reference for a pure-Rust SQL database architecture:

```
executor/
├── operators/    — Volcano-style pull-based operators
├── parallel.rs   — Parallel execution (Rayon)
└── expression/   — Expression VM

optimizer/
├── cost.rs       — Cost model with I/O and CPU costs
├── join.rs       — Join optimization (dynamic programming)
├── bloom.rs      — Bloom filter propagation
└── aqe.rs        — Adaptive query execution
```

**What to borrow (conceptually, not code):**
- Volcano-style iterator model: each operator implements `fn next() -> Option<Row>`
- Cost-based optimizer with cardinality estimation
- Parallel execution with Rayon work-stealing
- MVCC with snapshot isolation (we need this for concurrent reads/writes)
- The clean `api/` → `parser/` → `optimizer/` → `executor/` → `storage/` pipeline

**What we do DIFFERENTLY:**
- Stoolap is SQL/relational. We need graph-specific operators:
  - `Expand` (follow relationships with depth/direction)
  - `VarLengthExpand` (BFS/DFS with min..max depth)
  - `ShortestPath` (Dijkstra/BFS with cycle detection)
  - `PatternMatch` (multi-hop pattern matching)
- Our storage isn't row-oriented — it's fingerprint-oriented via ladybug-rs
- We add Hamming-distance operators that have no SQL equivalent

### BORROW: Connection Architecture (from official neo4j crate)

The official driver has the most mature connection handling:

- **Driver** manages a connection pool (no need to pool drivers)
- **Sessions** are cheap, borrow connections from pool as needed
- **Three execution paths:**
  1. `Driver::execute_query()` — simplest, most optimizable
  2. `Session::transaction()` — full control with managed transactions
  3. `Session::auto_commit()` — for `CALL {} IN TRANSACTION`
- **Causal consistency** via bookmarks (abstract tokens representing DB state)
- **Retry with exponential backoff** for cluster resilience

**What to borrow for our Bolt backend:**
- The Driver → Session → Transaction hierarchy
- Bookmark management for causal consistency
- ExponentialBackoff retry strategy
- The `RoutingControl::Read` / `RoutingControl::Write` distinction
- Connection pool with health checking

---

## 14. Integration Contract: neo4j-rs ↔ ladybug-rs

This section defines what the `StorageBackend` trait needs to become so that
ladybug-rs can implement it faithfully AND neo4j-rs remains 100% truthful to
Neo4j 5.26.0 semantics.

### The Orchestration Model

```
User Cypher → neo4j-rs (ALWAYS parses, plans, orchestrates)
                 │
                 ├── For each LogicalPlan operator:
                 │     calls StorageBackend trait methods
                 │
                 ├── StorageBackend::Memory   → HashMap (test oracle)
                 ├── StorageBackend::Bolt     → sends Cypher to real Neo4j
                 └── StorageBackend::Ladybug  → calls ladybug-rs internals
                                                   │
                                                   ├── Lance tables (persistence)
                                                   ├── BindSpace (16K fingerprints)
                                                   ├── DN-Sparse CSR (adjacency)
                                                   └── DataFusion (query engine)
```

neo4j-rs is **always** the orchestrator: it parses Cypher, builds the plan,
and walks the plan tree making trait calls. Backends never see Cypher strings
(except through `execute_raw()` escape hatch for Bolt passthrough).

### Gap A: ARCHITECTURE.md Promises vs Actual Trait

The ARCHITECTURE.md defines a trait with methods the actual `storage/mod.rs`
**does not have yet**:

| Method | In ARCHITECTURE.md | In storage/mod.rs | Status |
|--------|-------------------|-------------------|--------|
| `connect(config)` | Yes | No | **MISSING** — needed for LadybugBackend init (open Lance, warm cache) |
| `execute_raw(tx, query, params)` | Yes | No | **MISSING** — escape hatch for Bolt passthrough & DataFusion pushdown |

### Gap B: Neo4j 5.26.0 Operations the Trait Lacks

Auditing the current 22 trait methods against the full Cypher statement types
in `ast.rs` (Query, Create, Merge, Delete, Set, Schema) reveals these gaps:

#### B1. Relationship Property CRUD (blocks `SET r.prop`, `REMOVE r.prop`)

```rust
// MISSING — needed for: SET r.weight = 0.5
async fn set_relationship_property(&self, tx: &mut Self::Tx, id: RelId, key: &str, val: Value) -> Result<()>;

// MISSING — needed for: REMOVE r.weight
async fn remove_relationship_property(&self, tx: &mut Self::Tx, id: RelId, key: &str) -> Result<()>;
```

The AST has `SetItem::Property` which can target either nodes or relationships.
The execution engine needs these methods to handle relationship writes.

#### B2. DETACH DELETE (blocks `DETACH DELETE n`)

```rust
// MISSING — needed for: DETACH DELETE n (atomically delete node + all rels)
async fn detach_delete_node(&self, tx: &mut Self::Tx, id: NodeId) -> Result<bool>;
```

The AST has `DeleteClause { detach: bool }`. Without this, the executor must
manually enumerate and delete each relationship first — which is both slow on
ladybug-rs (N individual Lance deletes vs one batch) and non-atomic.

#### B3. MERGE / Upsert (blocks `MERGE (n:Person {name: 'Ada'})`)

```rust
// MISSING — needed for: MERGE (n:L {match_props}) ON CREATE SET ... ON MATCH SET ...
async fn merge_node(
    &self,
    tx: &mut Self::Tx,
    labels: &[&str],
    match_props: PropertyMap,     // properties to match on
    on_create: PropertyMap,        // properties to set if created
    on_match: Vec<(&str, Value)>,  // properties to update if matched
) -> Result<(NodeId, bool)>;       // (id, was_created)
```

The AST has `MergeClause { on_create, on_match }`. Without atomic merge,
the executor composes `nodes_by_property()` + `create_node()`, but this has
TOCTOU race conditions under concurrent writes. ladybug-rs with DataFusion
can do atomic upserts in a single Lance operation.

#### B4. Shortest Path (blocks `shortestPath()`, `allShortestPaths()`)

```rust
// MISSING — needed for: shortestPath((a)-[*]-(b))
async fn shortest_path(
    &self,
    tx: &Self::Tx,
    src: NodeId,
    dst: NodeId,
    dir: Direction,
    rel_types: &[&str],
    max_depth: Option<usize>,
) -> Result<Option<Path>>;

// MISSING — needed for: allShortestPaths((a)-[*]-(b))
async fn all_shortest_paths(
    &self,
    tx: &Self::Tx,
    src: NodeId,
    dst: NodeId,
    dir: Direction,
    rel_types: &[&str],
    max_depth: Option<usize>,
) -> Result<Vec<Path>>;
```

The current `expand()` returns ALL paths to a depth — it doesn't find shortest
paths between two specific endpoints. ladybug-rs can accelerate this massively
via Hamming-guided BFS (HDR cascade prunes 90% of candidates).

#### B5. Scan Gaps (blocks several MATCH patterns)

```rust
// MISSING — needed for: MATCH (n) RETURN n (no label filter)
async fn all_nodes(&self, tx: &Self::Tx) -> Result<Vec<Node>>;

// MISSING — needed for: MATCH ()-[r:KNOWS]->() RETURN r
async fn relationships_by_type(&self, tx: &Self::Tx, rel_type: &str) -> Result<Vec<Relationship>>;

// MISSING — needed for: WHERE n.age > 25 AND n.age < 65 (range predicates)
async fn nodes_by_property_range(
    &self,
    tx: &Self::Tx,
    label: &str,
    key: &str,
    min: Option<&Value>,  // None = unbounded lower
    max: Option<&Value>,  // None = unbounded upper
) -> Result<Vec<Node>>;
```

Range queries are critical. Currently `nodes_by_property()` only does exact
equality on ONE property. Neo4j uses B-tree indexes for range scans. ladybug-rs
would use DataFusion's filter pushdown.

#### B6. Batch Operations (performance-critical for ladybug-rs)

```rust
// MISSING — needed for efficient plan execution with IN lists
async fn get_nodes(&self, tx: &Self::Tx, ids: &[NodeId]) -> Result<Vec<Option<Node>>>;

// MISSING — needed for bulk import / UNWIND
async fn create_nodes_batch(
    &self,
    tx: &mut Self::Tx,
    nodes: Vec<(&[&str], PropertyMap)>,  // (labels, props) per node
) -> Result<Vec<NodeId>>;
```

For ladybug-rs this is the difference between 1 Lance read (batch) vs N
individual reads. Lance is columnar — batch is 100-1000x faster.

#### B7. Schema Constraints (blocks `CREATE CONSTRAINT`)

```rust
// MISSING — needed for: CREATE CONSTRAINT FOR (n:Person) REQUIRE n.email IS UNIQUE
async fn create_constraint(
    &self,
    label: &str,
    property: &str,
    constraint_type: ConstraintType,
) -> Result<()>;

async fn drop_constraint(&self, label: &str, property: &str) -> Result<()>;

async fn list_indexes(&self, tx: &Self::Tx) -> Result<Vec<IndexInfo>>;
async fn list_constraints(&self, tx: &Self::Tx) -> Result<Vec<ConstraintInfo>>;
```

The AST already has `SchemaCommand::CreateConstraint` and `DropConstraint` —
but the trait has no way to execute them. Neo4j 5.26.0 supports UNIQUE, NODE
KEY, EXISTENCE, and RELATIONSHIP TYPE existence constraints.

#### B8. Degree Counting (performance optimization)

```rust
// MISSING — needed for: size((n)-->()) without materializing all rels
async fn degree(
    &self,
    tx: &Self::Tx,
    node: NodeId,
    dir: Direction,
    rel_type: Option<&str>,
) -> Result<u64>;
```

Currently `get_relationships()` returns all relationships just to count them.
ladybug-rs can answer degree queries from DN-Sparse CSR metadata without
touching Lance at all.

### Gap C: What ladybug-rs Must Expose (the Integration Surface)

For ladybug-rs to implement `StorageBackend`, it needs to provide:

```
ladybug-rs public API (what neo4j-rs imports):
├── LadybugBackend          — struct implementing StorageBackend
├── LadybugTx               — struct implementing Transaction
├── LadybugConfig            — initialization config (data_dir, cache_size, etc.)
└── Error conversions        — ladybug errors → neo4j-rs Error
```

#### What ladybug-rs must handle internally:

| neo4j-rs Calls | ladybug-rs Translates To |
|----------------|--------------------------|
| `create_node(labels, props)` | Lance INSERT into nodes table + BindSpace fingerprint alloc |
| `get_node(id)` | Lance point lookup + JSON→PropertyMap deserialize |
| `get_nodes(ids)` | Lance batch read (single I/O, columnar) |
| `nodes_by_label(label)` | DataFusion: `SELECT * FROM nodes WHERE labels @> [label]` |
| `nodes_by_property(label, k, v)` | DataFusion: `SELECT * FROM nodes WHERE labels @> [label] AND json_extract(properties, k) = v` |
| `nodes_by_property_range(...)` | DataFusion: `SELECT * ... WHERE json_extract(...) BETWEEN min AND max` |
| `expand(node, dir, types, depth)` | Hamming-guided BFS on DN-Sparse + HDR cascade pruning |
| `shortest_path(src, dst, ...)` | Hamming-guided bidirectional BFS (HDR cascade distance estimate) |
| `create_index(label, prop, type)` | Lance: create secondary index; for Vector: IVF-PQ on fingerprints |
| `merge_node(labels, match_props, ...)` | DataFusion: atomic INSERT ... ON CONFLICT via Lance transactions |
| `detach_delete_node(id)` | Lance: batch delete from nodes + relationships WHERE src_id=id OR dst_id=id |
| `execute_raw(query, params)` | DataFusion: run SQL directly (escape hatch) |
| `begin_tx(mode)` | Lance: begin transaction (copy-on-write) |
| `commit_tx(tx)` | Lance: commit version |

#### What ladybug-rs must NOT expose:

- BindSpace internals (16K fingerprint arrays)
- DN-Sparse CSR format
- HDR cascade levels
- DataFusion execution details
- holograph types

All of these stay behind the `StorageBackend` trait boundary.

### Gap D: Transaction Trait Needs Enrichment

The current `Transaction` trait is too thin:

```rust
// Current — just mode + id
pub trait Transaction: Send + Sync {
    fn mode(&self) -> TxMode;
    fn id(&self) -> TxId;
}
```

For Neo4j 5.26.0 compatibility, it needs:

```rust
pub trait Transaction: Send + Sync {
    fn mode(&self) -> TxMode;
    fn id(&self) -> TxId;

    // Causal consistency — Neo4j bookmarks
    fn bookmark(&self) -> Option<&str>;

    // Database targeting — Neo4j multi-tenancy
    fn database(&self) -> Option<&str>;

    // Timeout — prevent runaway queries
    fn timeout(&self) -> Option<std::time::Duration>;
}
```

ladybug-rs would use `bookmark()` for Lance version pinning (snapshot reads).
Bolt would forward bookmarks to the Neo4j server. Memory can ignore them.

### Gap E: Capability Negotiation

Different backends support different features. The execution engine needs to
know what it can push down vs what it must handle itself:

```rust
/// What a backend can handle natively.
pub trait BackendCapabilities {
    /// Can the backend handle filtered scans with arbitrary predicates?
    fn supports_predicate_pushdown(&self) -> bool;

    /// Can the backend handle shortestPath natively?
    fn supports_shortest_path(&self) -> bool;

    /// Can the backend handle aggregation (COUNT, SUM, etc.)?
    fn supports_aggregation_pushdown(&self) -> bool;

    /// Can the backend run raw queries? (Bolt: yes, Memory: no)
    fn supports_raw_query(&self) -> bool;

    /// Can the backend do atomic MERGE? (Ladybug: yes, Memory: no)
    fn supports_atomic_merge(&self) -> bool;

    /// Does the backend support vector similarity search?
    fn supports_vector_search(&self) -> bool;
}
```

The execution engine uses this to make optimization decisions:
- Memory: handle everything in the executor (no pushdown)
- Bolt: push entire query via `execute_raw()` (maximum pushdown)
- Ladybug: push filtered scans and shortest paths, handle CASE/UNWIND in executor

### Summary: Required Changes by Priority

```
MUST HAVE (blocks Cypher compliance):
  ├── set_relationship_property()        — blocks SET on relationships
  ├── remove_relationship_property()     — blocks REMOVE on relationships
  ├── detach_delete_node()               — blocks DETACH DELETE
  ├── all_nodes()                        — blocks MATCH (n) with no label
  ├── relationships_by_type()            — blocks MATCH ()-[r:T]->()
  ├── connect(config)                    — blocks any non-Memory backend init
  ├── create_constraint() / drop_...     — blocks schema commands in AST
  └── list_indexes() / list_constraints() — blocks SHOW INDEXES/CONSTRAINTS

SHOULD HAVE (needed for faithful semantics):
  ├── merge_node()                       — blocks MERGE (compositional fallback exists)
  ├── shortest_path() / all_...          — blocks shortestPath() function
  ├── nodes_by_property_range()          — blocks WHERE n.age > 25
  ├── execute_raw()                      — blocks Bolt passthrough
  ├── Transaction::bookmark()            — blocks causal consistency
  └── Transaction::database()            — blocks multi-tenancy

NICE TO HAVE (performance, ladybug-rs benefits massively):
  ├── get_nodes() (batch)                — 100-1000x for ladybug-rs
  ├── create_nodes_batch()               — bulk import
  ├── degree()                           — avoid materializing rels
  └── BackendCapabilities trait          — optimizer decisions
```

---

## 15. What to Work On

> **Note**: Section 14 above identifies trait gaps that must be addressed.
> Factor these into the priorities below — specifically, the "MUST HAVE" items
> from Gap B should be implemented alongside the execution engine (Priority 3).

### Phase 1 Priorities (Current Focus)

#### Priority 1: Cypher Parser (`src/cypher/parser.rs`)

The lexer and AST types are done. What's missing is the recursive descent
parser that turns `Vec<Token>` into a `Statement`.

Start with these Cypher forms:

1. `MATCH (n:Label) RETURN n` — basic node scan with return
2. `MATCH (a)-[:TYPE]->(b) RETURN a, b` — relationship pattern
3. `MATCH (n) WHERE n.prop = $value RETURN n` — WHERE clause with expressions
4. `CREATE (n:Label {key: value})` — node creation
5. `CREATE (a)-[:TYPE]->(b)` — relationship creation
6. `MATCH (n) SET n.prop = value` — property update
7. `MATCH (n) DELETE n` — deletion
8. `MATCH (n) RETURN n ORDER BY n.name LIMIT 10` — ordering + pagination

Cross-reference the Neo4j Java parser at
`neo4j/community/cypher/front-end/` in the
[reference repo](https://github.com/AdaWorldAPI/neo4j) (branch `release/5.26.0`).

#### Priority 2: Planner (`src/planner/mod.rs`)

Map each `Statement` variant to a `LogicalPlan` tree:

- `Query` with `MATCH (n:Label)` → `NodeScan { label, alias }`
- `MATCH (a)-[:T]->(b)` → `NodeScan` + `Expand`
- `WHERE expr` → wrap in `Filter { predicate }`
- `RETURN items` → `Project { items }`
- `CREATE (n:L {props})` → `CreateNode`
- `LIMIT n` → `Limit`
- `ORDER BY` → `Sort`

#### Priority 3: Execution Engine (`src/execution/mod.rs`)

Implement Volcano-style pull-based execution. Each `LogicalPlan` variant maps
to `StorageBackend` calls:

```
NodeScan("Person")  →  backend.nodes_by_label(tx, "Person")
Expand(a, OUTGOING) →  backend.get_relationships(tx, a.id, Outgoing, ...)
Filter(predicate)   →  evaluate predicate against each row
Project(items)      →  extract requested columns
CreateNode(...)     →  backend.create_node(tx, labels, props)
```

#### Priority 4: Integration Tests

Create `tests/` directory with end-to-end tests:

```
tests/
├── parse_tests.rs          # Cypher string → AST validation
├── e2e_memory_tests.rs     # Full pipeline against MemoryBackend
└── value_round_trip.rs     # Create → query → verify for every Value variant
```

### Phase 3 Priorities (Bolt Protocol)

#### Priority 5: Bolt Protocol

See [Ecosystem section above](#steal-packstream-serde-from-neo4rs) for what to
borrow from `neo4rs`.

### Borrowing Priority Order

```
P0 (do first):
  ├── PackStream serde (from neo4rs) — needed for Bolt backend
  ├── Bolt message catalog (from neo4rs) — HELLO/BEGIN/RUN/PULL/COMMIT
  └── BoltBytesBuilder test helper — for unit testing

P1 (do next):
  ├── ValueSend/ValueReceive split (from official driver)
  ├── Connection pool + TLS (from neo4rs connection.rs)
  └── Volcano executor model (from Stoolap conceptually)

P2 (do later):
  ├── Routing + bookmarks (from official driver)
  ├── Integration test patterns (from neo4rs tests/)
  └── Cost-based optimizer (from Stoolap conceptually)

P3 (future):
  ├── Retry + resilience (from official driver)
  ├── MVCC (from Stoolap, adapted for graph)
  └── Parallel execution (from Stoolap/Rayon)
```

---

## 16. Reference Resources

### Authoritative

| Resource | Use For |
|----------|---------|
| [AdaWorldAPI/neo4j](https://github.com/AdaWorldAPI/neo4j) (branch `release/5.26.0`) | Cypher grammar, Bolt format, TCK tests, semantics |
| [openCypher spec](https://opencypher.org/) | Language specification |

### Ecosystem (What to Borrow)

| Resource | Use For |
|----------|---------|
| [neo4rs](https://github.com/neo4j-labs/neo4rs) | PackStream serde, Bolt messages, integration test patterns |
| [neo4j crate](https://github.com/robsdedude/neo4j-rust-driver) | ValueSend/ValueReceive split, routing, bookmarks |
| [Stoolap](https://github.com/stoolap/stoolap) | Volcano executor model, cost-based optimizer, MVCC |
| [PackStream blog post](https://neo4j.com/blog/developer/python-driver-10x-faster-with-rust/) | Performance insights — PackStream is the hot path |

See [docs/INSPIRATION.md](INSPIRATION.md) for the full deep-dive analysis.

### Sibling Projects

| Crate | Role | Boundary |
|-------|------|----------|
| [holograph](https://github.com/AdaWorldAPI/holograph) | Bitpacked vector primitives (Hamming, GraphBLAS, HDR cascade) | DO NOT import into neo4j-rs |
| [ladybug-rs](https://github.com/AdaWorldAPI/ladybug-rs) | Hamming-accelerated storage engine (16K fingerprints, LanceDB, DataFusion) | Import only behind `ladybug` feature |

---

## 17. What NOT to Do

- Do NOT add `holograph` as a direct dependency
- Do NOT add `arrow` or `datafusion` to the default feature set
- Do NOT make the parser async
- Do NOT store backend-specific data in model types
- Do NOT skip the MemoryBackend tests ("it works on Bolt" is not enough)
- Do NOT implement APOC procedures (that's a future extension crate)
- Do NOT add Redis, HTTP, or gRPC server code (that's a separate binary crate)
- Do NOT use `anyhow` in library code (use `thiserror`)
- Do NOT import `cypher/` types into `storage/` or vice versa
- Do NOT leak Arrow, Lance, or holograph types into the core API
- Do NOT use `unsafe` for zero-copy deserialization without profiling first
- Do NOT hand-roll connection pooling (use `deadpool` or `bb8`)
- Do NOT use openssl for TLS (use `rustls` — no C dependency)
