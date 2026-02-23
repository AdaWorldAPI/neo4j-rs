# neo4j-rs — The Clean Graph Database Trinity

> **Goal**: A clean-room Rust reimplementation of Neo4j's graph model with zero
> technical debt, two storage layers, and full Cypher compatibility.
>
> **Author**: AdaWorldAPI
> **Date**: 2026-02-13

---

## The Three Crates

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          USER / APPLICATION                                 │
│                                                                             │
│   Cypher query string    OR    Rust Builder API    OR    MCP / Flight       │
│         │                          │                         │              │
│         ▼                          ▼                         ▼              │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │                        neo4j-rs (crate)                             │    │
│  │                                                                     │    │
│  │  • Cypher parser + planner + optimizer                              │    │
│  │  • Transaction manager (ACID, MVCC)                                 │    │
│  │  • Property graph model (Node, Relationship, Path)                  │    │
│  │  • Native Neo4j Bolt wire protocol client                           │    │
│  │  • StorageBackend trait → pluggable backends                        │    │
│  │  • Result streaming via Arrow RecordBatch                           │    │
│  │                                                                     │    │
│  │  This is THE public API. Clean. Typed. Zero legacy.                 │    │
│  └──────────────┬───────────────────────────────────┬──────────────────┘    │
│                 │                                   │                       │
│        ┌────────▼────────┐                ┌─────────▼─────────┐            │
│        │  StorageBackend │                │  StorageBackend    │            │
│        │  ::LadybugRs    │                │  ::Neo4jBolt       │            │
│        └────────┬────────┘                └─────────┬─────────┘            │
│                 │                                   │                       │
│  ┌──────────────▼───────────────────┐    ┌──────────▼──────────────────┐   │
│  │        ladybug-rs (crate)        │    │   External Neo4j Server     │   │
│  │                                  │    │   (via Bolt protocol)       │   │
│  │  • Cypher → DataFusion SQL       │    │                             │   │
│  │  • 16384-bit fingerprint nodes   │    │   Pass-through for          │   │
│  │  • Hamming-accelerated traversal │    │   existing Neo4j clusters   │   │
│  │  • DN-Tree addressing            │    └────────────────────────────┘   │
│  │  • LanceDB persistence           │                                     │
│  │  • DataFusion query engine        │                                     │
│  │                                  │                                     │
│  └──────────────┬───────────────────┘                                     │
│                 │                                                          │
│        ┌────────▼────────┐                                                │
│        │   holograph      │                                                │
│        │   (dependency)   │                                                │
│        │                  │                                                │
│        │  • bitpack ops   │                                                │
│        │  • Hamming SIMD  │                                                │
│        │  • GraphBLAS     │                                                │
│        │  • blasgraph     │  ← RedisGraph-compatible sparse matrices       │
│        │  • HDR cascade   │                                                │
│        │  • DN-Sparse     │                                                │
│        └──────────────────┘                                                │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Separation of Concerns

### neo4j-rs — The Holy Grail Interface

**What it IS:**
- A clean property graph database interface in Rust
- Full Cypher parser (openCypher spec compliant)
- Query planner with cost-based optimization
- ACID transaction semantics with MVCC
- Bolt wire protocol client (connect to real Neo4j)
- `StorageBackend` trait for pluggable storage

**What it is NOT:**
- A storage engine (that's ladybug-rs or external Neo4j)
- A vector database (that's holograph through ladybug-rs)
- Tightly coupled to any single backend

**Design principle**: If you took neo4j-rs and pointed it at a real Neo4j server
via Bolt, it should behave identically to pointing it at ladybug-rs. The
interface IS the contract.

### ladybug-rs — The Hamming-Flavored Storage Engine

**What it IS:**
- The actual storage engine implementing `StorageBackend`
- Cypher → DataFusion SQL transpilation
- 16384-bit fingerprint nodes (8192 semantic + 8192×n metadata/edges)
- Hamming-accelerated graph traversal (90% pruning via HDR cascade)
- LanceDB + DataFusion for persistence and query
- DN-Tree hierarchical addressing

**What it is NOT:**
- A user-facing API (that's neo4j-rs)
- A Cypher parser (that's neo4j-rs)
- A standalone binary for end users

### holograph — The Bitpacked Primitives

**What it IS:**
- Pure Rust bitpacked vector operations
- SIMD-accelerated Hamming distance
- GraphBLAS sparse matrix algebra (blasgraph)
- HDR cascade for hierarchical search
- DN-Sparse node storage
- The blasgraph module exposes RedisGraph-compatible operations

**blasgraph specifically:**
- GrBMatrix/GrBVector with XOR semirings
- Sparse COO/CSR storage on Arrow
- RedisGraph-like GRAPH.QUERY interface via GraphBLAS ops
- This is what "John Doe" uses if they want redisgraph-like behavior

---

## The StorageBackend Trait (The Contract)

```rust
// neo4j-rs/src/storage/mod.rs

/// The single contract between neo4j-rs and any storage engine.
/// This is the "holy grail" — clean, typed, zero technical debt.
#[async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    type Tx: Transaction;

    // === Lifecycle ===
    // NOTE: connect() is NOT a trait method. Each backend provides its own
    // associated function (e.g., MemoryBackend::new(), BoltBackend::connect()).
    async fn shutdown(&self) -> Result<()>;

    // === Transactions ===
    async fn begin_tx(&self, mode: TxMode) -> Result<Self::Tx>;
    async fn commit_tx(&self, tx: Self::Tx) -> Result<()>;
    async fn rollback_tx(&self, tx: Self::Tx) -> Result<()>;

    // === Node Operations ===
    async fn create_node(&self, tx: &mut Self::Tx, labels: &[&str], props: PropertyMap) -> Result<NodeId>;
    async fn get_node(&self, tx: &Self::Tx, id: NodeId) -> Result<Option<Node>>;
    async fn delete_node(&self, tx: &mut Self::Tx, id: NodeId) -> Result<bool>;
    async fn set_node_property(&self, tx: &mut Self::Tx, id: NodeId, key: &str, val: Value) -> Result<()>;
    async fn remove_node_property(&self, tx: &mut Self::Tx, id: NodeId, key: &str) -> Result<()>;
    async fn add_label(&self, tx: &mut Self::Tx, id: NodeId, label: &str) -> Result<()>;
    async fn remove_label(&self, tx: &mut Self::Tx, id: NodeId, label: &str) -> Result<()>;

    // === Relationship Operations ===
    async fn create_relationship(&self, tx: &mut Self::Tx, src: NodeId, dst: NodeId,
                                  rel_type: &str, props: PropertyMap) -> Result<RelId>;
    async fn get_relationship(&self, tx: &Self::Tx, id: RelId) -> Result<Option<Relationship>>;
    async fn delete_relationship(&self, tx: &mut Self::Tx, id: RelId) -> Result<bool>;

    // === Traversal ===
    async fn get_relationships(&self, tx: &Self::Tx, node: NodeId,
                                dir: Direction, rel_type: Option<&str>) -> Result<Vec<Relationship>>;
    async fn expand(&self, tx: &Self::Tx, node: NodeId, dir: Direction,
                    rel_types: &[&str], depth: ExpandDepth) -> Result<Vec<Path>>;

    // === Index ===
    async fn create_index(&self, label: &str, property: &str, index_type: IndexType) -> Result<()>;
    async fn drop_index(&self, label: &str, property: &str) -> Result<()>;

    // === Schema ===
    async fn node_count(&self, tx: &Self::Tx) -> Result<u64>;
    async fn relationship_count(&self, tx: &Self::Tx) -> Result<u64>;
    async fn labels(&self, tx: &Self::Tx) -> Result<Vec<String>>;
    async fn relationship_types(&self, tx: &Self::Tx) -> Result<Vec<String>>;

    // === Query (escape hatch for native queries) ===
    // Default returns Error::ExecutionError("not supported"). Bolt overrides.
    async fn execute_raw(&self, tx: &Self::Tx, query: &str, params: ParamMap) -> Result<QueryResult>;
}

/// Transaction trait
#[async_trait]
pub trait Transaction: Send + Sync {
    fn mode(&self) -> TxMode;
    fn id(&self) -> TxId;
}
```

---

## LanceDB as Acceleration Cache

The key insight: neo4j-rs doesn't know or care about LanceDB. That's an
implementation detail of ladybug-rs. But the pattern matters:

```
┌──────────────────────────────────────────────────────────────────┐
│                     ladybug-rs internals                          │
│                                                                   │
│  ┌─────────────────────────────────┐                             │
│  │    Property Storage (Lance)      │                             │
│  │                                  │                             │
│  │  Table: nodes                    │                             │
│  │    id:          UInt64           │                             │
│  │    labels:      Utf8[]           │                             │
│  │    fingerprint: FixedBinary(2048)│  ← 16384-bit vector        │
│  │    properties:  Binary (JSON)    │                             │
│  │    created_at:  Timestamp        │                             │
│  │    version:     UInt64           │                             │
│  │                                  │                             │
│  │  Table: relationships            │                             │
│  │    id:          UInt64           │                             │
│  │    src_id:      UInt64           │                             │
│  │    dst_id:      UInt64           │                             │
│  │    rel_type:    Utf8             │                             │
│  │    fingerprint: FixedBinary(2048)│  ← edge as vector          │
│  │    properties:  Binary (JSON)    │                             │
│  │                                  │                             │
│  └─────────────┬───────────────────┘                             │
│                │                                                  │
│                │  Lance handles:                                  │
│                │  • Persistence (Parquet/S3)                      │
│                │  • Versioning (time travel)                      │
│                │  • ANN index (IVF-PQ on fingerprints)            │
│                │  • Column pruning                                │
│                │                                                  │
│  ┌─────────────▼───────────────────┐                             │
│  │    Hot Cache (holograph memory)  │                             │
│  │                                  │                             │
│  │  BindSpace: N × 256 u64 arrays  │  ← in-memory fingerprints  │
│  │  DN-Sparse CSR adjacency        │  ← in-memory graph topo    │
│  │  HDR Cascade sketches           │  ← multi-level Hamming     │
│  │                                  │                             │
│  │  Hot cache handles:              │                             │
│  │  • O(1) Hamming distance         │                             │
│  │  • 90% candidate pruning         │                             │
│  │  • XOR bind/unbind for traversal │                             │
│  │  • Schema-predicate filtering    │                             │
│  └──────────────────────────────────┘                             │
│                                                                   │
│  DataFusion sits on top of both:                                  │
│  • TableProvider for Lance tables                                 │
│  • TableProvider for hot BindSpace                                │
│  • UDFs: hamming_distance, xor_bind, schema_passes               │
│  • Optimizer: pushes HDR cascade below sort                       │
│  • Cypher → SQL transpilation with UDFs                           │
└──────────────────────────────────────────────────────────────────┘
```

### What LanceDB accelerates that Neo4j does terribly:

1. **Vector similarity** — Neo4j has no native Hamming. Lance + HDR cascade = orders of magnitude faster
2. **Property filtering + graph traversal combined** — Neo4j scans all matching nodes, then traverses. Lance schema-filtered scan prunes during I/O
3. **Batch operations** — Neo4j is record-at-a-time. Lance is columnar batch. 1000x for analytics
4. **Time travel** — Neo4j versioning is expensive. Lance versions are free (copy-on-write Parquet)
5. **Cold storage** — Neo4j keeps everything in memory or page cache. Lance → S3 for cold tiers

### What stays faithful Neo4j:

1. **Cypher syntax** — 100% openCypher compatible parsing
2. **Graph semantics** — MATCH patterns, variable-length paths, shortest path
3. **ACID transactions** — BEGIN/COMMIT/ROLLBACK with proper isolation
4. **Index-backed lookups** — CREATE INDEX → label+property index
5. **Bolt protocol** — can proxy to real Neo4j seamlessly

---

## Repository Structure

### neo4j-rs (new repo: AdaWorldAPI/neo4j-rs)

```
neo4j-rs/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   │
│   ├── model/                    # Property graph model (DTOs)
│   │   ├── mod.rs
│   │   ├── node.rs               # Node { id, labels, properties }
│   │   ├── relationship.rs       # Relationship { id, src, dst, type, properties }
│   │   ├── path.rs               # Path (sequence of nodes + rels)
│   │   ├── value.rs              # Value enum (Bool, Int, Float, String, List, Map, ...)
│   │   └── property_map.rs       # PropertyMap = HashMap<String, Value>
│   │
│   ├── cypher/                   # Cypher language
│   │   ├── mod.rs
│   │   ├── lexer.rs              # Tokenizer
│   │   ├── parser.rs             # AST construction
│   │   ├── ast.rs                # Cypher AST types
│   │   ├── semantic.rs           # Semantic analysis + type checking
│   │   └── pretty.rs             # AST → Cypher string (for debugging)
│   │
│   ├── planner/                  # Query planning
│   │   ├── mod.rs
│   │   ├── logical.rs            # Logical plan (Scan, Expand, Filter, Project, ...)
│   │   ├── physical.rs           # Physical plan (IndexScan, HashJoin, ...)
│   │   ├── optimizer.rs          # Cost-based optimization rules
│   │   └── cost.rs               # Cost model
│   │
│   ├── execution/                # Query execution
│   │   ├── mod.rs
│   │   ├── engine.rs             # Pull-based execution engine
│   │   ├── operators.rs          # Physical operators
│   │   └── result.rs             # QueryResult + streaming
│   │
│   ├── storage/                  # Storage abstraction
│   │   ├── mod.rs                # StorageBackend trait
│   │   ├── bolt.rs               # Neo4j Bolt protocol backend
│   │   └── ladybug.rs            # ladybug-rs backend (feature-gated)
│   │
│   ├── tx/                       # Transaction management
│   │   ├── mod.rs
│   │   ├── manager.rs            # TxManager
│   │   └── isolation.rs          # Isolation levels
│   │
│   └── index/                    # Index management
│       ├── mod.rs
│       ├── btree.rs              # B-tree property index
│       └── label.rs              # Label index
│
├── tests/
│   ├── cypher_parser_tests.rs
│   ├── cypher_e2e_tests.rs       # Full round-trip: parse → plan → execute
│   ├── tck/                      # openCypher TCK (Technology Compatibility Kit)
│   └── bolt_compat_tests.rs
│
└── benches/
    └── cypher_bench.rs
```

### Changes to holograph (existing repo)

```
holograph/
├── src/
│   ├── graphblas/                # EXISTING — this IS blasgraph
│   │   ├── mod.rs                # Already has GrBMatrix, semirings
│   │   ├── sparse.rs             # COO/CSR on Arrow
│   │   ├── matrix.rs             # GrBMatrix ops
│   │   └── ops.rs                # mxm, vxm, eWiseAdd, etc.
│   │
│   ├── blasgraph/                # NEW — RedisGraph-compatible facade
│   │   ├── mod.rs                # GRAPH.QUERY / GRAPH.DELETE / etc.
│   │   ├── graph.rs              # BlasGraph struct (named graph)
│   │   ├── commands.rs           # RedisGraph command parser
│   │   ├── result_set.rs         # RedisGraph result format
│   │   └── resp.rs               # RESP protocol encoding (optional)
│   │
│   └── ... (everything else unchanged)
```

### Changes to ladybug-rs (existing repo)

```
ladybug-rs/
├── src/
│   ├── backend/                  # NEW — implements neo4j-rs StorageBackend
│   │   ├── mod.rs
│   │   ├── storage_impl.rs       # impl StorageBackend for LadybugBackend
│   │   ├── tx_impl.rs            # impl Transaction for LadybugTx
│   │   ├── node_ops.rs           # Node CRUD via BindSpace + Lance
│   │   ├── rel_ops.rs            # Relationship CRUD via DN-Sparse + Lance
│   │   ├── traversal.rs          # expand() via Hamming-accelerated BFS/DFS
│   │   ├── index_ops.rs          # Index management
│   │   └── cypher_to_df.rs       # Cypher logical plan → DataFusion physical plan
│   │
│   └── ... (everything else stays)
```

---

## Clean DTO Layer

The entire system communicates through clean, serializable DTOs:

```rust
// neo4j-rs/src/model/value.rs

/// Universal value type — matches Neo4j's type system exactly
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(HashMap<String, Value>),
    Node(Node),
    Relationship(Relationship),
    Path(Path),
    // Temporal types (Neo4j compatible)
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(DateTime<Utc>),
    LocalDateTime(NaiveDateTime),
    Duration(Duration),
    // Spatial types
    Point2D { srid: i32, x: f64, y: f64 },
    Point3D { srid: i32, x: f64, y: f64, z: f64 },
}

// neo4j-rs/src/model/node.rs

/// A node in the property graph
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub labels: Vec<String>,
    pub properties: PropertyMap,
}

// neo4j-rs/src/model/relationship.rs

/// A relationship (edge) in the property graph
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub id: RelId,
    pub src: NodeId,
    pub dst: NodeId,
    pub rel_type: String,
    pub properties: PropertyMap,
}
```

These DTOs are what flows between neo4j-rs and any backend. ladybug-rs
internally converts to/from its 16384-bit fingerprints, but that's invisible
to neo4j-rs.

---

## Expansion Path

```
Phase 1 (now):  neo4j-rs with Bolt backend only
                → Connect to existing Neo4j, validate Cypher parser
                → openCypher TCK tests pass

Phase 2:        neo4j-rs + ladybug-rs backend
                → StorageBackend::LadybugRs works
                → Same Cypher queries work on both backends
                → Hamming acceleration for similarity queries

Phase 3:        holograph/blasgraph for RedisGraph compat
                → GRAPH.QUERY interface on holograph's GraphBLAS
                → "John Doe" can use it like RedisGraph

Phase 4:        Advanced Cypher
                → CALL procedures for Hamming search, HDR cascade
                → Vector similarity as first-class Cypher extension
                → DN-path addressing in MATCH patterns
```

---

## Why This Avoids Technical Debt

1. **Trait-first design** — StorageBackend is the contract. Implementations
   can be swapped, tested independently, evolved separately.

2. **Clean DTO boundary** — Node/Relationship/Value cross the boundary.
   No holograph types, no Lance types, no Arrow types in neo4j-rs.

3. **Parser owns nothing** — The Cypher parser produces an AST. It doesn't
   know about storage, execution, or optimization. Pure function.

4. **Planner is backend-agnostic** — Logical plans use abstract operators.
   The physical plan adapts to the backend's capabilities.

5. **ladybug-rs is an implementation detail** — All the 16K fingerprint
   magic, HDR cascade, DN-Tree addressing lives behind the trait.
   neo4j-rs never sees it.

6. **holograph is a library** — Pure functions, zero I/O, zero state.
   ladybug-rs owns the storage; holograph provides the math.

7. **Bolt backend validates correctness** — Run the same queries against
   real Neo4j and against ladybug-rs. Results must match. This is the
   ultimate regression test.
