# What to Borrow — Ecosystem Analysis for neo4j-rs

## Sources Analyzed

| Source | What It Is | Key Insight |
|--------|-----------|-------------|
| **neo4rs** (neo4j-labs) | Community Rust Bolt driver, 279★ | PackStream serde, BoltStruct macros, integration test patterns |
| **neo4j** (robsdedude/docs.rs) | Official-adjacent Rust driver, 15★ | ValueSend/ValueReceive split, routing, bookmark management, session lifecycle |
| **Python Rust ext** (neo4j blog) | PackStream rewrite in Rust for 10x perf | **THE hot path**: serde-based PackStream is where ALL performance lives |
| **Stoolap** | Embedded SQL db in pure Rust, 0.2 | Volcano executor, cost-based optimizer, MVCC, full architecture reference |

---

## 1. STEAL: PackStream Serde (from neo4rs)

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
- The entire PackStream binary format: marker bytes (0x80=tiny_string, 0x90=tiny_list,
  0xA0=tiny_map, 0xB0=struct, 0xC0=null, etc.)
- The serde-based approach: `impl Serializer for PackStreamSerializer`,
  `impl Deserializer for PackStreamDeserializer`
- The `BoltBytesBuilder` test helper — invaluable for unit testing Bolt messages
- The `#[derive(BoltStruct)] #[signature(0xB3, 0x4E)]` proc macro for Node/Relationship

**What to improve:**
- neo4rs has two separate type hierarchies (BoltNode vs Row::Node) — we unify at DTO boundary
- Their deserialization does `unsafe { Box::from_raw() }` for zero-copy — profile first, add later
- Support Bolt 5.x (neo4rs only supports 4.0-4.3 currently!)

---

## 2. STEAL: ValueSend/ValueReceive Split (from official neo4j crate)

The `neo4j` crate on docs.rs (by robsdedude, the same person who built the Python Rust ext!)
has an elegant type split:

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
- The ValueSend/ValueReceive distinction (adapt to our Value/ValueOut or similar)
- The `value_map!` macro pattern
- Their routing + bookmark management patterns (causal consistency across sessions)

---

## 3. STEAL: Bolt Protocol Messages (from neo4rs)

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

Each message uses serde for serialization with structure tags.
The handshake negotiation, chunking (MAX_CHUNK_SIZE = 65535 - 2), and
TLS setup are all working and tested.

**What to borrow:**
- The complete message catalog with their binary signatures
- Chunked transfer encoding (u16 length prefix per chunk, 0x0000 terminator)
- Connection pool management (deadpool-based in neo4rs)
- Version negotiation handshake (4-byte magic + supported version ranges)

---

## 4. BORROW: Volcano-Style Executor (from Stoolap)

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

---

## 5. BORROW: Connection Architecture (from official neo4j crate)

The official driver has the most mature connection handling:

- **Driver** manages a connection pool (no need to pool drivers)
- **Sessions** are cheap, borrow connections from pool as needed
- **Three execution paths:**
  1. `Driver::execute_query()` — simplest, most optimizable
  2. `Session::transaction()` — full control with managed transactions
  3. `Session::auto_commit()` — for CALL {} IN TRANSACTION
- **Causal consistency** via bookmarks (abstract tokens representing DB state)
- **Retry with exponential backoff** for cluster resilience

**What to borrow for our Bolt backend:**
- The Driver → Session → Transaction hierarchy
- Bookmark management for causal consistency
- ExponentialBackoff retry strategy
- The `RoutingControl::Read` / `RoutingControl::Write` distinction
- Connection pool with health checking

---

## 6. BORROW: Integration Test Patterns (from neo4rs)

neo4rs has excellent integration tests in `integrationtests/tests/`:

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

**What to borrow:**
- The test structure: each concern isolated in its own file
- Container-based testing (they use testcontainers with Neo4j Docker)
- Round-trip testing pattern: create → query → verify for every type
- Edge case coverage: unbounded relationships, missing properties, etc.

---

## 7. KEY ARCHITECTURAL DECISIONS

### From neo4rs + official driver:
1. **Use `bytes::Bytes`** for zero-copy PackStream — not `Vec<u8>`
2. **Serde for PackStream** — don't hand-roll serialization
3. **Connection pooling via deadpool** (or bb8) — not hand-rolled
4. **TLS via rustls** — not openssl (no C dependency)
5. **tokio::BufStream** for connection I/O — not raw TcpStream

### From Stoolap:
6. **Volcano-style pull operators** for the execution engine
7. **Cost-based optimizer** with cardinality estimation
8. **Clean pipeline**: parser → planner → optimizer → executor → storage

### From the Python Rust ext blog:
9. **PackStream is THE hot path** — optimize this above everything else
10. **Profile before optimizing** — their 10x came from a very specific bottleneck

### Our unique additions:
11. **StorageBackend trait** — neither driver nor embedded DB has this
12. **Hamming-accelerated traversal** — unique to ladybug-rs
13. **Fingerprint-based pattern matching** — prune 90% before touching properties
14. **Graph-specific operators** — Expand, VarLengthExpand, ShortestPath

---

## Priority Order for Borrowing

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
