# Development Guide — neo4j-rs

> Practical guide for contributors. For architecture decisions see
> [ARCHITECTURE.md](../ARCHITECTURE.md). For ecosystem analysis see
> [INSPIRATION.md](INSPIRATION.md).

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Building](#building)
3. [Testing](#testing)
4. [Project Status](#project-status)
5. [Module Walkthrough](#module-walkthrough)
6. [Data Flow](#data-flow)
7. [Adding Code: Patterns to Follow](#adding-code-patterns-to-follow)
8. [Error Handling](#error-handling)
9. [Feature Flags](#feature-flags)
10. [Coding Conventions](#coding-conventions)
11. [What to Work On](#what-to-work-on)
12. [Reference Resources](#reference-resources)

---

## Prerequisites

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

### Sibling Repositories (optional)

If you need the `ladybug` feature, clone the sibling repo next to this one:

```
parent/
├── neo4j-rs/          # this repo
└── ladybug-rs/        # git clone https://github.com/AdaWorldAPI/ladybug-rs
```

The `ladybug` dependency uses `path = "../ladybug-rs"` in `Cargo.toml`.

---

## Building

```bash
# Fast compile check (no codegen, catches all errors)
cargo check

# Full build (default features only — no bolt, no ladybug, no arrow)
cargo build

# Build with Bolt protocol support
cargo build --features bolt

# Build everything
cargo build --features full
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

## Testing

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

### Test Structure

Tests live in two places:

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

### Dev Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` (full features) | Async test runtime |
| `pretty_assertions` | Colored diff on test failures |
| `proptest` | Property-based / fuzz testing |

---

## Project Status

### What's Done

| Component | File(s) | Status |
|-----------|---------|--------|
| Public API (`Graph<B>`) | `src/lib.rs` | Complete |
| Error types | `src/lib.rs` | Complete |
| Node, Relationship, Path DTOs | `src/model/` | Complete |
| Value enum (26 variants) | `src/model/value.rs` | Complete |
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

---

## Module Walkthrough

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
- **`Value`** — 17-variant enum covering Neo4j's full type system
- **`PropertyMap`** — `HashMap<String, Value>`
- **`NodeId` / `RelId`** — Newtype wrappers around `u64`
- **`Direction`** — `Outgoing | Incoming | Both`

Every public type derives `Debug`, `Clone`, `Serialize`, `Deserialize`.

### `src/cypher/` — Parser (Pure Functions)

The parser module has zero imports from storage or execution. It is
synchronous with no I/O.

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
[What to Work On](#what-to-work-on).

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

## Data Flow

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

## Adding Code: Patterns to Follow

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

## Error Handling

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

## Feature Flags

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

## Coding Conventions

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

## What to Work On

Current priority is **Phase 1: Complete the Cypher Parser**, followed by
**Phase 2: Wire up the execution engine**.

### Priority 1: Cypher Parser (`src/cypher/parser.rs`)

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

Cross-reference the Neo4j Java parser at:
`neo4j/community/cypher/front-end/` in the
[reference repo](https://github.com/AdaWorldAPI/neo4j) (branch `release/5.26.0`).

### Priority 2: Planner (`src/planner/mod.rs`)

Map each `Statement` variant to a `LogicalPlan` tree:

- `Query` with `MATCH (n:Label)` → `NodeScan { label, alias }`
- `MATCH (a)-[:T]->(b)` → `NodeScan` + `Expand`
- `WHERE expr` → wrap in `Filter { predicate }`
- `RETURN items` → `Project { items }`
- `CREATE (n:L {props})` → `CreateNode`
- `LIMIT n` → `Limit`
- `ORDER BY` → `Sort`

### Priority 3: Execution Engine (`src/execution/mod.rs`)

Implement Volcano-style pull-based execution. Each `LogicalPlan` variant maps
to `StorageBackend` calls:

```
NodeScan("Person")  →  backend.nodes_by_label(tx, "Person")
Expand(a, OUTGOING) →  backend.get_relationships(tx, a.id, Outgoing, ...)
Filter(predicate)   →  evaluate predicate against each row
Project(items)      →  extract requested columns
CreateNode(...)     →  backend.create_node(tx, labels, props)
```

### Priority 4: Integration Tests

Create `tests/` directory with end-to-end tests:

```
tests/
├── parse_tests.rs          # Cypher string → AST validation
├── e2e_memory_tests.rs     # Full pipeline against MemoryBackend
└── value_round_trip.rs     # Create → query → verify for every Value variant
```

### Priority 5: Bolt Protocol (Phase 3)

See [INSPIRATION.md](INSPIRATION.md) for what to borrow from `neo4rs`:
- PackStream serde serialization (the hot path)
- Bolt message catalog (HELLO, BEGIN, RUN, PULL, COMMIT)
- BoltBytesBuilder test helper
- Connection pool and TLS via `rustls`

---

## Reference Resources

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

See [docs/INSPIRATION.md](INSPIRATION.md) for detailed analysis of each source.

### Sibling Projects

| Crate | Role | Boundary |
|-------|------|----------|
| [holograph](https://github.com/AdaWorldAPI/holograph) | Bitpacked vector primitives | DO NOT import into neo4j-rs |
| [ladybug-rs](https://github.com/AdaWorldAPI/ladybug-rs) | Hamming-accelerated storage engine | Import only behind `ladybug` feature |

---

## What NOT to Do

- Do NOT add `holograph` as a direct dependency
- Do NOT add `arrow` or `datafusion` to the default feature set
- Do NOT make the parser async
- Do NOT store backend-specific data in model types
- Do NOT skip MemoryBackend tests ("it works on Bolt" is not enough)
- Do NOT implement APOC procedures (future extension crate)
- Do NOT add Redis, HTTP, or gRPC server code (separate binary crate)
- Do NOT use `anyhow` in library code (use `thiserror`)
- Do NOT import `cypher/` types into `storage/` or vice versa
- Do NOT leak Arrow, Lance, or holograph types into the core API
