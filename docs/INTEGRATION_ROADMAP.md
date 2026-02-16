# neo4j-rs Integration Roadmap

> **Updated**: 2026-02-16 | **Branch**: `claude/add-development-guide-PK55e`
> **Scope**: neo4j-rs + ladybug-rs + holograph trinity + crewai-rust + n8n-rs ecosystem
> **Principle**: The StorageBackend trait is the integration seam. All backends
> speak the same contract. All Cypher works identically on every backend.
>
> **See also**:
> - [`CAM_CYPHER_REFERENCE.md`](CAM_CYPHER_REFERENCE.md) — Full CAM address map (0x200-0x2FF) for Cypher ops
> - [`GUI_PLAN.md`](GUI_PLAN.md) — egui/eframe GUI architecture and panel layout
> - [`FEATURE_MATRIX.md`](FEATURE_MATRIX.md) — Current feature status scorecard
> - [`STRATEGY_INTEGRATION_PLAN.md`](STRATEGY_INTEGRATION_PLAN.md) — Multi-crate StrategicNode unification
> - [`REALITY_CHECK.md`](REALITY_CHECK.md) — Honest audit of gaps and known issues
> - [`COMPATIBILITY_REPORT.md`](COMPATIBILITY_REPORT.md) — Neo4j 5.26.0 compatibility analysis

---

## Current State Summary

```
Component          Status    LOC     Tests   What Works
─────────────────  ────────  ──────  ──────  ────────────────────────────────────
Cypher Lexer       Done      435     8       58 token kinds, comments, escapes
Cypher Parser      Done      1,374   20      Full openCypher: MATCH/CREATE/SET/DELETE
Cypher AST         Done      278     —       7 statement types, full expressions
Planner            Working   436     —       18 logical operators, basic optimizer
Executor           Working   1,171   —       Volcano-style pull model, expression eval
Memory Backend     Working   624     8       CRUD + traversal + label index
Model Types        Done      530     3       Node, Relationship, Path, Value (18 types)
Transaction Layer  Stub      20      —       API exists, no isolation or rollback
Index Layer        Stub      16      —       Types defined, create_index is no-op
Bolt Backend       —         0       —       Feature-gated, not started
Ladybug Backend    —         0       —       Feature-gated, designed but not started
─────────────────  ────────  ──────  ──────  ────────────────────────────────────
Total                        ~5,400  116     Core pipeline: parse → plan → execute
```

---

## Phase Overview

```
Phase 1  ██████████████████████░░  90%   Cypher Parser + Memory Backend
Phase 2  █████████████░░░░░░░░░░  55%   Execution Engine Hardening
Phase 3  ████░░░░░░░░░░░░░░░░░░  15%   Bolt Protocol Client
Phase 4  ███░░░░░░░░░░░░░░░░░░░  10%   ladybug-rs LadybugBackend
Phase 5  ░░░░░░░░░░░░░░░░░░░░░░   0%   GUI + Developer Tools
Phase 6  ░░░░░░░░░░░░░░░░░░░░░░   0%   Advanced Cypher + TCK
Phase 7  ░░░░░░░░░░░░░░░░░░░░░░   0%   Ecosystem Unification
```

---

## Phase 1: Cypher Parser + Memory Backend (90%)

**Status**: Core pipeline works end-to-end. 116 tests passing (112 run + 4 ignored).

The full pipeline is operational: a Cypher string parses into an AST, plans into a
LogicalPlan, and executes against MemoryBackend to return real results. CREATE, SET,
DELETE, DETACH DELETE, aggregations (COUNT/SUM/COLLECT), ORDER BY, SKIP, LIMIT, and
DISTINCT all work.

### What's Done

- Lexer: 58 token kinds, block/line comments, string escapes, `$param` support
- Parser: recursive descent, MATCH/OPTIONAL MATCH/WHERE/RETURN/WITH/UNWIND/CREATE/
  DELETE/DETACH DELETE/SET/REMOVE/CALL...YIELD, schema commands (parsed only)
- Patterns: node `(n:Label {props})`, relationship `-[r:Type]->`, multi-type `[:T|:U]`,
  variable-length `*1..3` (parsed, not executed)
- Expressions: arithmetic, comparison, logical (short-circuit), string ops, CASE, IN,
  IS NULL, property access with null propagation
- AST: 7 statement types, full expression hierarchy
- Memory backend: CRUD for nodes/relationships, adjacency lists, label index, batch ops

### Remaining Work

| # | Task | Est. LOC | Priority | Blocks |
|---|------|:--------:|:--------:|--------|
| 1.1 | Implement missing scalar functions: `type()`, `keys()`, `properties()`, `size()`, `coalesce()` | ~150 | **High** | Executor completeness |
| 1.2 | Implement missing aggregation functions: `avg()`, `min()`, `max()` | ~80 | **High** | Executor completeness |
| 1.3 | Implement string functions: `toLower()`, `toUpper()`, `trim()`, `substring()`, `replace()`, `split()` | ~80 | Medium | String-heavy queries |
| 1.4 | Implement type conversion functions: `toInteger()`, `toFloat()`, `toString()`, `toBoolean()` | ~50 | Medium | Type coercion queries |
| 1.5 | Implement list functions: `head()`, `tail()`, `last()`, `range()`, `reverse()`, `size()` | ~60 | Medium | List manipulation |
| 1.6 | Implement `MERGE` statement planning + execution (get-or-create semantics) | ~200 | Medium | Upsert patterns |
| 1.7 | Implement variable-length path execution (`*1..3`, `*`, `*2..`) | ~300 | Medium | Graph traversal depth |
| 1.8 | Implement `UNION` / `UNION ALL` execution | ~100 | Low | Set operations |
| 1.9 | Implement regex matching (`=~` operator) | ~50 | Low | Pattern matching |

### Known Gaps (from REALITY_CHECK.md)

- `create_index()` is a silent no-op — no actual indexes exist
- `rollback_tx()` is a no-op — writes are immediately applied
- MemoryBackend has no MVCC — concurrent writes are unsound
- `property_index` field in MemoryBackend is allocated but never used (dead code)

### Definition of Done

- All openCypher core clauses parse, plan, and execute on MemoryBackend
- 150+ tests passing
- Zero `todo!()` or `unimplemented!()` in the hot path
- All 4 currently-ignored tests pass (UNWIND, UNION)

---

## Phase 2: Execution Engine Hardening (55%)

**Goal**: Bring the executor to production quality with real transactions, working
indexes, and a cost-based optimizer.

### 2A: Real Transactions for MemoryBackend

The current MemoryBackend applies writes immediately. `rollback_tx()` is a no-op.
`commit_tx()` is a no-op. This means no atomicity, no isolation, no durability.
Only consistency (via `delete_node` checking for relationships) exists.

| # | Task | Est. LOC | Depends On | Risk |
|---|------|:--------:|:----------:|------|
| 2A.1 | Implement write-ahead log (WAL) for MemoryBackend: `MemoryTx` holds `Vec<Mutation>`, `commit_tx` replays them | ~400 | — | Medium: requires rearchitecting write path |
| 2A.2 | Add snapshot isolation: copy-on-write HashMap per transaction, readers see consistent snapshots | ~300 | 2A.1 | High: subtle correctness issues |
| 2A.3 | Make `rollback_tx()` actually discard buffered mutations | ~200 | 2A.1 | Low |
| 2A.4 | Add `element_id: Option<String>` to Node/Relationship for Bolt compatibility | ~50 | — | Low |
| 2A.5 | Change `ResultRow` from `HashMap` to `IndexMap` to preserve column order | ~80 | — | Low |
| 2A.6 | Add `committed: bool` flag + `Drop` warning to `ExplicitTx` | ~50 | — | Low |
| 2A.7 | Consolidate MemoryBackend locks: single `RwLock<MemoryState>` instead of per-collection locks | ~200 | — | Medium: eliminates race window in `delete_node` |

### 2B: Property Indexes

Currently `IndexType` is defined as an enum (BTree, FullText, Unique, Vector) but
no actual index structures exist. `create_index()` returns `Ok(())` and does nothing.

| # | Task | Est. LOC | Depends On | Risk |
|---|------|:--------:|:----------:|------|
| 2B.1 | Implement BTree index: `HashMap<(Label, Property), BTreeMap<Value, Vec<NodeId>>>` | ~300 | — | Low |
| 2B.2 | Wire `create_index()` / `drop_index()` to maintain actual BTree structures | ~100 | 2B.1 | Low |
| 2B.3 | Auto-update indexes on `create_node()`, `set_property()`, `delete_node()` | ~150 | 2B.1 | Medium |
| 2B.4 | Planner: recognize `WHERE n.prop = val` and emit `IndexLookup` instead of scan + filter | ~150 | 2B.1, 2B.2 | Medium |
| 2B.5 | Unique constraint enforcement on create/set (reject duplicate values) | ~100 | 2B.1 | Low |
| 2B.6 | `nodes_by_property_range()` for range scans (`WHERE n.age > 30`) | ~100 | 2B.1 | Low |

### 2C: Cost-Based Optimizer

The current optimizer only does LIMIT pushdown. No statistics, no cost model,
no predicate pushdown, no join ordering.

| # | Task | Est. LOC | Depends On | Risk |
|---|------|:--------:|:----------:|------|
| 2C.1 | Collect basic statistics: label cardinality, property selectivity, relationship type counts | ~200 | — | Low |
| 2C.2 | Cost model: estimate output rows per operator (cardinality estimation) | ~300 | 2C.1 | High: accuracy is hard |
| 2C.3 | Join ordering: pick optimal Expand sequence in multi-hop MATCH | ~200 | 2C.2 | High |
| 2C.4 | Predicate pushdown: move Filter below Expand when possible | ~150 | 2C.2 | Medium |
| 2C.5 | Index selection: choose between scan and index lookup based on selectivity | ~100 | 2B.4, 2C.2 | Medium |

### Definition of Done

- `rollback_tx()` actually undoes mutations
- Concurrent reads during a write transaction see consistent snapshots
- Property indexes accelerate `WHERE n.prop = val` lookups
- Planner chooses index scan vs. full scan based on cardinality estimates
- `EXPLAIN` query prefix shows the chosen logical plan

---

## Phase 3: Bolt Protocol Client (15%)

**Goal**: Connect to external Neo4j 5.x as a correctness oracle and enable neo4j-rs
as a drop-in Rust driver for existing Neo4j deployments.

### Architecture

```
neo4j-rs                          External Neo4j 5.x
   |                                    |
   +-- Cypher -> AST -> Plan           |
   |                                    |
   +-- BoltBackend --PackStream------> Bolt port 7687
   |     |                              |
   |     +-- Handshake (Bolt 5.x)      |
   |     +-- HELLO + AUTH              |
   |     +-- RUN + PULL                |
   |     +-- BEGIN/COMMIT/ROLLBACK     |
   |                                    |
   +-- Results <--PackStream---------- |
```

### Reference Implementations

| Source | What To Borrow | Notes |
|--------|---------------|-------|
| **neo4rs** (neo4j-labs) | PackStream serde, `#[derive(BoltStruct)]` pattern, `BoltBytesBuilder` test helper | Only supports Bolt 4.0-4.3; we need 5.x |
| **robsdedude driver** | `ValueSend/ValueReceive` split, connection pool, causal bookmarks, routing | Publishes the `neo4j` crate on crates.io |
| **Neo4j Java source** | Bolt 5.x message catalog, PackStream v2 spec | `neo4j/community/bolt/` in reference repo |

### Implementation Tasks

| # | Task | Est. LOC | Depends On | Reference |
|---|------|:--------:|:----------:|-----------|
| 3.1 | PackStream encoder/decoder with serde integration | ~500 | — | neo4rs `packstream/` |
| 3.2 | Bolt message types: HELLO, LOGON, RUN, PULL, DISCARD, BEGIN, COMMIT, ROLLBACK, GOODBYE, RESET | ~400 | 3.1 | neo4rs `bolt/request/` |
| 3.3 | TCP connection + TLS + Bolt version negotiation handshake | ~300 | 3.1, 3.2 | neo4rs `stream.rs` |
| 3.4 | Authentication: HELLO + LOGON with basic/bearer/custom schemes | ~100 | 3.3 | |
| 3.5 | `impl StorageBackend for BoltBackend` — translate trait calls to Bolt RUN+PULL | ~600 | 3.3, 3.4 | |
| 3.6 | Transaction management over Bolt: BEGIN/COMMIT/ROLLBACK with real ACID | ~200 | 3.5 | |
| 3.7 | Connection pool: `deadpool`-based or custom, with configurable limits | ~300 | 3.5 | robsdedude driver |
| 3.8 | Causal consistency via bookmarks (abstract tokens for DB state) | ~150 | 3.6 | robsdedude driver |
| 3.9 | Routing: Read/Write distinction for cluster deployments | ~200 | 3.7 | robsdedude driver |
| 3.10 | Cross-backend test harness: same query against Memory + Bolt, assert identical results | ~400 | 3.5 | |
| 3.11 | Value translation: `neo4j-rs Value <-> PackStream Value` round-trip | ~200 | 3.1 | |

### Key Design Decisions

- **Bolt version**: Target 5.x (Neo4j 5.26.0 from reference repo)
- **ValueSend/ValueReceive split**: Users can't send Node objects as parameters (graph entities are output-only)
- **Streaming**: Results stream via PULL with configurable fetch size (not buffered all-at-once)
- **Feature gate**: All Bolt code behind `bolt` feature flag; adds `tokio` + `bytes` deps

### PackStream: The Hot Path

Neo4j's Python driver team rewrote PackStream in Rust and got 3-10x speedups.
PackStream is the binary serialization format for all Bolt communication:
marker byte + length + data. Profile this FIRST when optimizing.

### Definition of Done

- Can connect to Neo4j 5.x, authenticate, and run arbitrary Cypher queries
- All E2E tests pass on both Memory and Bolt backends with identical results
- Connection pooling with configurable min/max connections
- Transactions over Bolt have real ACID semantics
- Causal consistency via bookmarks

---

## Phase 4: ladybug-rs LadybugBackend (10%)

**Goal**: Implement `StorageBackend` for ladybug-rs, translating Cypher operations
to CAM-addressed container operations on 8192-bit CogRecords.

> **Full CAM address reference**: [`CAM_CYPHER_REFERENCE.md`](CAM_CYPHER_REFERENCE.md)
> Source: `ladybug-rs/src/learning/cam_ops.rs` (4776 lines, 4096 ops, Cypher at 0x200-0x2FF)

### Architecture

```
neo4j-rs Cypher Query
   |
   +-- Parse -> Plan -> Execute
   |
   +-- LadybugBackend
         |
         +-- NodeId(u64) <-> PackedDn (7-level hierarchy)
         |
         +-- create_node()
         |     +-- Fingerprint from labels + properties
         |     +-- Write to CAM: type_ns(label) | fingerprint_prefix
         |     +-- CogRecord: W0=PackedDn, W40-47=label Bloom, Content=property FP
         |
         +-- expand()
         |     +-- Inline edge walk: W16-31 (64 max direct)
         |     +-- CSR overflow: W96-111 (12 max overflow)
         |     +-- Bloom pre-filter: W40-47 neighbor membership
         |
         +-- MATCH -> scan(type_id, prefix) on Arrow buffers
         |     +-- L1 scent index (1.25 KB, ~50ns): 98.8% elimination
         |     +-- L2 scent index (320 KB): 99.997% elimination
         |     +-- SIMD Hamming on remaining candidates
         |
         +-- vector_query() -> CAKES k-NN
               +-- Belichtungsmesser 7-point sampling (~14 cycles)
               +-- Full SIMD Hamming on survivors
               +-- O(k * 2^LFD * log(n)) sublinear guarantee
```

### Prerequisites from ladybug-rs

Before implementing LadybugBackend in neo4j-rs, these must exist in ladybug-rs:

| Prerequisite | Status | Location |
|-------------|--------|----------|
| `StrategicNode` zero-copy view over CogRecord | Designed | `src/cognitive/strategy.rs` (to be created) |
| Stable `CogRecord` API for word reads/writes | Exists | `crates/ladybug-contract/src/meta.rs` |
| BindSpace surface/fluid/node zone API | Exists | `src/storage/bind_space.rs` |
| CAKES k-NN search | Exists | Via holograph |
| 10-layer thinking_style in W12-15 | Designed | Pending layout change (7->10 layers) |
| SPO crystal for strategy triples | Exists | `src/extensions/spo/spo.rs` |

### Implementation Tasks

| # | Task | Est. LOC | Depends On | Location |
|---|------|:--------:|:----------:|----------|
| 4.1 | `NodeId <-> PackedDn` bidirectional address translation | ~100 | — | `src/storage/ladybug.rs` |
| 4.2 | `fingerprint_from_node()` — deterministic fingerprint from labels + properties | ~200 | ladybug-rs contract | `src/storage/ladybug.rs` |
| 4.3 | `verb_from_rel_type()` — map relationship types to 144 Go verbs + hash fallback | ~100 | ladybug-rs contract | `src/storage/ladybug.rs` |
| 4.4 | `impl StorageBackend for LadybugBackend` — node CRUD | ~300 | 4.1, 4.2 | `src/storage/ladybug.rs` |
| 4.5 | Relationship CRUD via inline edges (W16-31) + CSR overflow (W96-111) | ~200 | 4.3, 4.4 | `src/storage/ladybug.rs` |
| 4.6 | `expand()` via inline edge walk + Bloom pre-filter (W40-47) | ~200 | 4.5 | `src/storage/ladybug.rs` |
| 4.7 | `vector_query()` via CAKES k-NN + SIMD Hamming | ~150 | ladybug-rs CAKES API | `src/storage/ladybug.rs` |
| 4.8 | `call_procedure()` routing to cognitive modules (thinking_styles, debate, etc.) | ~200 | ladybug-rs cognitive API | `src/storage/ladybug.rs` |
| 4.9 | Label scan via scent index: L1 (1.25KB, ~50ns) + L2 (320KB) elimination | ~150 | ladybug-rs scent API | `src/storage/ladybug.rs` |
| 4.10 | Cross-backend test harness: same query against Memory + Ladybug, assert identical results | ~400 | 4.4-4.8 | `tests/` |

### Translation Contract

| Neo4j Concept | Ladybug Equivalent | Container Location |
|---------------|-------------------|-------------------|
| `Node` with labels + properties | 8192-bit CogRecord | 1 KB metadata + 1 KB content |
| `NodeId(u64)` | `PackedDn` (7-level hierarchy) | W0 |
| `Relationship` type | One of 144 verbs or hash-fallback | W16-31 inline edges |
| Property access | Metadata field or content fingerprint | Word-level read |
| Label membership | Bloom filter | W40-47 |
| Graph traversal | Inline edge walk + Bloom | W16-31 + W40-47 |
| Vector similarity | CAKES k-NN + Hamming | Content container |
| NARS truth values | Frequency + confidence | W4-7 |

### Procedure Registry (ladybug-native)

| Procedure | Description | CAM Operation |
|-----------|-------------|---------------|
| `ladybug.similar(node, k)` | CAKES k-nearest neighbors | SIMD Hamming scan |
| `ladybug.bind(a, verb, b)` | Create ABBA-retrievable edge | XOR binding |
| `ladybug.unbind(bound, key)` | Recover bound component | XOR unbinding |
| `ladybug.causal_trace(effect, depth)` | Reverse causal trace | NARS truth propagation |
| `ladybug.debate(pro, con)` | Structured argument | Inner Council majority vote |
| `ladybug.collapse_gate(candidates)` | Quantum-like assessment | Gate state evaluation |
| `ladybug.thinking_styles(query)` | 12 diverse reasoning paths | FieldModulation dispatch |
| `db.index.vector.queryNodes` | Neo4j 5.x standard vector search | Routes to CAKES |

### Cypher Operation -> CAM Address Mapping

| Cypher Operation | CAM Address | Internal Mechanism | Performance |
|-----------------|:-----------:|-------------------|-------------|
| `MATCH (n:Label)` | `0x200` | Scent index scan on Arrow buffers | ~50ns L1 scent |
| `WHERE n.prop = val` | `0x203` | Fingerprint prefix match | Content container SIMD |
| `MATCH (a)-[:T]->(b)` | `0x201` | Inline edge walk (W16-31) + Bloom (W40-47) | ~14 cycles Belichtungsmesser |
| `CREATE (n:Label {})` | `0x220` | BindSpace write + fingerprint generation | |
| `RETURN n.prop` | `0x2E0` | Container metadata word read | Zero-copy |
| `shortestPath(...)` | `0x260` | HammingMinPlus semiring | Sublinear via scent |
| Vector similarity | `0x2C5` | `simd_scan(bucket)` -> CAKES k-NN | ~10ms at 7PB scale |

### Definition of Done

- All E2E tests pass on Memory, Bolt, AND Ladybug backends
- `vector_query()` returns real CAKES k-NN results
- Cypher `MATCH` uses scent index for sub-millisecond label scans
- No ladybug-rs internal types leak into neo4j-rs core
- Clean DTO boundary: only `Node`, `Relationship`, `Path`, `Value`, `PropertyMap` cross

---

## Phase 5: GUI + Developer Tools (0%)

**Goal**: Provide visual graph exploration, query tooling, and debugging support.

> **Full GUI architecture**: [`GUI_PLAN.md`](GUI_PLAN.md) — egui/eframe panels, async bridge, qualia heatmap

### 5A: Web-Based Graph Explorer (Primary Recommendation)

```
+--------------------------------------------------+
|  neo4j-rs Web GUI                                |
|                                                  |
|  +------------------------------------------+   |
|  |  Cypher Editor (CodeMirror + syntax hl)  |   |
|  |  > MATCH (n:Person)-[:KNOWS]->(m)        |   |
|  |  > RETURN n, m                           |   |
|  +------------------------------------------+   |
|                                                  |
|  +------------------------------------------+   |
|  |        Force-directed graph viz           |   |
|  |     O Alice --KNOWS--> O Bob             |   |
|  |         \                 |               |   |
|  |          KNOWS           KNOWS            |   |
|  |           \               |               |   |
|  |            O Carol ------>O Dave          |   |
|  +------------------------------------------+   |
|                                                  |
|  Properties: {name: "Alice", age: 30}           |
|  Labels: [:Person, :Employee]                    |
|  Relationships: 2 outgoing, 1 incoming          |
+--------------------------------------------------+
```

| # | Task | Technology | Est. LOC |
|---|------|-----------|:--------:|
| 5A.1 | HTTP API server with JSON endpoints | Rust + axum | ~500 |
| 5A.2 | `/api/query` endpoint (POST Cypher -> JSON result) | axum | ~100 |
| 5A.3 | `/api/graph` endpoint (GET subgraph as nodes + edges) | axum | ~200 |
| 5A.4 | Static file serving for frontend | axum | ~50 |
| 5A.5 | Frontend: Cypher editor with syntax highlighting | TypeScript + CodeMirror | ~400 |
| 5A.6 | Frontend: Force-directed graph visualization | TypeScript + d3-force / Cytoscape.js | ~600 |
| 5A.7 | Frontend: Property inspector panel | TypeScript | ~200 |
| 5A.8 | Frontend: Query history + saved queries | TypeScript | ~200 |
| 5A.9 | WebSocket for live query streaming | axum + tokio-tungstenite | ~300 |

**Recommended Stack**:
- Backend: `axum` (Tokio-native, ~200 lines for full API)
- Frontend: Vanilla TypeScript + `d3-force` for graph layout + `CodeMirror` for editor
- Bundler: `esbuild` (fast, no webpack complexity)

### 5B: Native Desktop GUI (Alternative)

| # | Task | Technology | Est. LOC |
|---|------|-----------|:--------:|
| 5B.1 | `egui` graph canvas with node/edge rendering | Rust + egui + eframe | ~800 |
| 5B.2 | Cypher input widget with basic completion | egui | ~200 |
| 5B.3 | Property table viewer | egui | ~150 |
| 5B.4 | Force-directed layout algorithm | Rust | ~300 |
| 5B.5 | Export to SVG/PNG | egui | ~100 |

**Pros**: Single binary, no web stack, native performance
**Cons**: Less polished than web, harder graph layout, smaller ecosystem

### 5C: TUI (Terminal UI)

| # | Task | Technology | Est. LOC |
|---|------|-----------|:--------:|
| 5C.1 | `ratatui` interactive Cypher REPL | Rust + ratatui | ~400 |
| 5C.2 | ASCII graph rendering | Rust | ~300 |
| 5C.3 | Table view for query results | ratatui | ~200 |
| 5C.4 | Command history + tab completion | Rust | ~150 |

**Pros**: Zero external deps, works over SSH, very Rust-native
**Cons**: Limited graph visualization, ASCII only

### Definition of Done

- At least one GUI option functional (web, desktop, or TUI)
- Can execute Cypher queries interactively
- Results displayed as both table and graph visualization
- Property inspector shows node/relationship details on click

---

## Phase 6: Advanced Cypher + TCK Compliance (0%)

**Goal**: Full openCypher compatibility validated against the official Technology
Compatibility Kit from the reference Neo4j 5.26.0 repo.

### 6A: TCK Harness

| # | Task | Est. LOC | Priority |
|---|------|:--------:|:--------:|
| 6A.1 | Extract openCypher TCK test cases from reference repo (`neo4j/community/cypher/`) | ~200 harness | **High** |
| 6A.2 | Build Gherkin/feature file runner for Rust (or adapt `cucumber-rs`) | ~400 | **High** |
| 6A.3 | Run TCK against MemoryBackend, triage failures | — | **High** |
| 6A.4 | Run TCK against BoltBackend (comparing our results vs. real Neo4j) | — | Medium |

### 6B: Advanced Cypher Features

| # | Task | Est. LOC | Priority | Notes |
|---|------|:--------:|:--------:|-------|
| 6B.1 | `shortestPath()` / `allShortestPaths()` | ~400 | **High** | BFS/Dijkstra with HammingMinPlus on ladybug |
| 6B.2 | `FOREACH` clause (imperative loop within write queries) | ~150 | Medium | |
| 6B.3 | `LOAD CSV` (parse CSV and bind rows to variables) | ~300 | Medium | |
| 6B.4 | `CALL { subquery }` (correlated subqueries) | ~300 | Medium | Neo4j 5.x feature |
| 6B.5 | List comprehensions: `[x IN list WHERE pred \| expr]` | ~200 | Medium | |
| 6B.6 | Pattern comprehensions: `[(a)-->(b) \| b.name]` | ~200 | Medium | |
| 6B.7 | Map projections: `n { .name, .age, score: 100 }` | ~100 | Low | |
| 6B.8 | `EXISTS { subquery }` — full subquery existence checks | ~200 | Medium | Currently returns error |
| 6B.9 | Temporal arithmetic: `date() + duration({months: 1})` | ~150 | Low | |
| 6B.10 | Full graph algorithms library (separate crate: `neo4j-algo`) | ~2000 | Low | PageRank, community detection, centrality |

### Definition of Done

- 80%+ of openCypher TCK test cases pass on MemoryBackend
- 95%+ pass on BoltBackend (validating against real Neo4j)
- `shortestPath()` works on all three backends
- All advanced syntax features parse, plan, and execute

---

## Phase 7: Ecosystem Unification (0%)

**Goal**: Integrate neo4j-rs into the broader Ada Consciousness ecosystem as the
Cypher query adapter, with ladybug-rs containers as the single source of truth.

> **Full design**: [`STRATEGY_INTEGRATION_PLAN.md`](STRATEGY_INTEGRATION_PLAN.md)

### Context

Four repositories have independent node representations that converge on the
same ladybug-rs CogRecord container:

| Repo | Node Type | Role |
|------|-----------|------|
| **neo4j-rs** | `Node { id, labels, properties }` | Cypher query adapter |
| **ladybug-rs** | `CogRecord` (8192-bit container) | Universal storage substrate |
| **crewai-rust** | `AgentBlueprint` + `ModuleInner` | Multi-agent cognitive profiles |
| **n8n-rs** | Workflow nodes | Orchestration and routing |

The unification principle: the ladybug-rs container IS the node. Neo4j-rs,
crewai-rust, and n8n-rs provide typed views over the same binary structure
rather than transcoding between formats.

### 7A: strategy.rs in ladybug-rs (Foundation)

| # | Task | File | Est. LOC | Depends On |
|---|------|------|:--------:|:----------:|
| 7A.1 | Expand W12-15 layer markers from 7 to 10 layers | `crates/ladybug-contract/src/meta.rs` | ~100 | — |
| 7A.2 | Add `StrategicNode` zero-copy view struct | `src/cognitive/strategy.rs` | ~300 | 7A.1 |
| 7A.3 | Implement thinking_style[10] <-> FieldModulation bridge | `src/cognitive/strategy.rs` | ~150 | 7A.2 |
| 7A.4 | Add TacticalCodebook with 10 chess + 10 AI War fingerprints | `src/cognitive/strategy.rs` | ~200 | 7A.2 |
| 7A.5 | Wire SPO crystal for strategy triples | `src/cognitive/strategy.rs` | ~100 | 7A.2 |
| 7A.6 | Implement self-modification protocol (recover_modulation -> propose_style_update) | `src/cognitive/strategy.rs` | ~200 | 7A.3 |
| 7A.7 | Tests for strategic node view, codebook, self-modification | `tests/strategy_tests.rs` | ~400 | 7A.2-7A.6 |

### 7B: crewai-rust Agency Bridge

| # | Task | File | Est. LOC | Depends On |
|---|------|------|:--------:|:----------:|
| 7B.1 | Add thinking_style field to AgentBlueprint struct | `src/meta_agents/types.rs` | ~30 | — |
| 7B.2 | Load thinking_style from module YAML into AgentBlueprint | `src/modules/loader.rs` | ~50 | 7B.1 |
| 7B.3 | Wire chess savant blueprints to use YAML thinking_styles | `src/meta_agents/savants.rs` | ~100 | 7B.1, 7B.2 |
| 7B.4 | Add style_update callback to InnerThoughtHook | `src/persona/inner_loop.rs` | ~100 | 7B.1 |
| 7B.5 | Connect PersonaProfile.self_modify to L10 crystallization feedback | `src/persona/inner_loop.rs` | ~150 | 7B.4, 7A.6 |
| 7B.6 | Expose `strategic_node_view()` from crewai-rust's ladybug bridge tools | `src/tools/chess/tools.rs` | ~100 | 7A.2, 7B.1 |

### 7C: n8n-rs Self-Orchestration

| # | Task | Est. LOC | Depends On |
|---|------|:--------:|:----------:|
| 7C.1 | Map n8n workflow nodes to strategic node containers | ~200 | 7A.2 |
| 7C.2 | Wire Q-values (W32-39) for workflow routing decisions | ~150 | 7C.1 |
| 7C.3 | Implement gate_state mapping (FLOW/HOLD/BLOCK -> workflow state) | ~100 | 7C.1 |
| 7C.4 | Add crystallization-based learning for workflow routing | ~200 | 7C.2, 7A.6 |

### 7D: neo4j-rs as Cypher Adapter

| # | Task | Est. LOC | Depends On |
|---|------|:--------:|:----------:|
| 7D.1 | Migrate aiwar.rs bridge concepts to TacticalCodebook seeds | ~100 | 7A.4 |
| 7D.2 | Cypher MATCH -> BindSpace resonance queries via LadybugBackend | ~300 | Phase 4 |
| 7D.3 | Cypher RETURN -> container metadata word reads (zero-copy) | ~200 | Phase 4 |
| 7D.4 | Cypher relationship traversal -> inline edge walks | ~200 | Phase 4 |
| 7D.5 | Cross-ecosystem test: Cypher query over agent+workflow+knowledge nodes | ~400 | 7D.1-7D.4 |

### Definition of Done

- All four repos use the same CogRecord container as single source of truth
- Neo4j-rs Cypher queries can traverse agent nodes, workflow nodes, and knowledge nodes
- thinking_style modulation feeds back through crystallization loop
- No transcoding between formats — typed views only

---

## Dependency Graph

```
Phase 1 (Parser + Memory) ......... 90% done
   |
   +----------+------------------+
   |          |                  |
   v          v                  v
Phase 2    Phase 3             Phase 5         Phase 6A
(Executor) (Bolt)              (GUI)           (TCK harness)
   |          |                  |                  |
   |          v                  |                  |
   |    Cross-backend            |                  |
   |     test harness            |                  |
   |          |                  |                  |
   +----+-----+                  |                  |
        |                        |                  |
        v                        |                  |
     Phase 4                     |                  |
   (Ladybug)                     |                  |
        |                        |                  |
        +--------+-------+------+---------+--------+
                 |       |                |
                 v       v                v
              Phase 7  Phase 6B       Full TCK
            (Ecosystem) (Advanced)   (compliance)
```

### Critical Path

```
Phase 1 remainder (functions + MERGE)
  -> Phase 2A (real transactions)
    -> Phase 3 (Bolt protocol)
      -> Phase 4 (ladybug-rs integration)
        -> Phase 7 (ecosystem unification)
```

### Parallel Work Streams

These can proceed independently of the critical path:

| Stream | Can Start | Dependencies |
|--------|-----------|-------------|
| Phase 2B: Indexes | Now | None |
| Phase 2C: Optimizer | After 2B | Needs index structures |
| Phase 5: GUI | Now | Only needs existing `graph.execute()` API |
| Phase 6A: TCK harness | Now | Run against MemoryBackend immediately |
| Phase 6B: Advanced Cypher | Now | Each feature is independent |

---

## Risk Assessment

### High Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| ladybug-rs API instability | LadybugBackend breaks on upstream changes | Pin to specific ladybug-rs version; define narrow contract |
| Bolt 5.x protocol complexity | Implementation takes longer than estimated | Start with Bolt 4.x (simpler), upgrade later; steal from neo4rs |
| MVCC correctness | Subtle bugs under concurrent access | Extensive property-based testing; compare against real Neo4j via Bolt |
| W12-15 layout change (7->10 layers) | Backward incompatibility in ladybug-rs | Coordinate with ladybug-rs team; version the layout |

### Medium Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| Cost-based optimizer accuracy | Wrong plans chosen, poor performance | Start with heuristic rules; add cost model incrementally |
| PackStream performance | Serialization becomes bottleneck | Profile early; use `bytes::Bytes` for zero-copy |
| openCypher TCK coverage | Low pass rate exposes many gaps | Prioritize most-used features; accept incremental compliance |

### Low Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| GUI framework choice | Wrong framework causes rework | Start with web (most flexible); defer desktop/TUI |
| `element_id` migration | Minor model change | Additive change (Option<String>), no breaking API |

---

## Estimated Effort by Phase

| Phase | Scope | Est. LOC | Effort | Can Start |
|-------|-------|:--------:|:------:|-----------|
| Phase 1 remainder | Missing functions + MERGE + var-length paths | ~1,070 | 2-3 days | Now |
| Phase 2A | Transactions + model fixes | ~1,280 | 1 week | Now |
| Phase 2B | Property indexes | ~750 | 1 week | Now |
| Phase 2C | Cost-based optimizer | ~950 | 1-2 weeks | After 2B |
| Phase 3 | Full Bolt protocol client | ~3,050 | 2-3 weeks | After Phase 1 |
| Phase 4 | ladybug-rs LadybugBackend | ~2,000 | 2-3 weeks | After Phase 3 |
| Phase 5A | Web GUI | ~2,550 | 1-2 weeks | Now |
| Phase 5C | TUI | ~1,050 | 1 week | Now |
| Phase 6A | TCK harness | ~600 | 3-5 days | Now |
| Phase 6B | Advanced Cypher features | ~2,200 | 2-3 weeks | Now (incremental) |
| Phase 7 | Ecosystem unification | ~3,230 | 3-4 weeks | After Phase 4 |
| **Total remaining** | | **~18,730** | | |

---

## Quick Reference: File Layout

```
neo4j-rs/
+-- CLAUDE.md              <- Development guide (coding standards, architecture rules)
+-- ARCHITECTURE.md        <- System design (trinity, separation of concerns)
+-- Cargo.toml             <- Edition 2024, rust-version 1.88, feature flags
+-- src/
|   +-- lib.rs             <- Public API: Graph<B>, Error, re-exports (240 LOC)
|   +-- model/             <- DTOs: Node, Relationship, Value, Path (530 LOC)
|   +-- cypher/            <- Parser: lexer -> AST, pure functions (2,100 LOC)
|   +-- planner/           <- Logical plan, basic optimizer (436 LOC)
|   +-- execution/         <- Volcano-style executor, expression eval (1,171 LOC)
|   +-- storage/           <- StorageBackend trait + implementations
|   |   +-- mod.rs         <- THE TRAIT (459 LOC, 31 methods)
|   |   +-- memory.rs      <- Reference in-memory implementation (624 LOC)
|   |   +-- bolt.rs        <- [Phase 3] Neo4j Bolt protocol (feature: bolt)
|   |   +-- ladybug.rs     <- [Phase 4] ladybug-rs backend (feature: ladybug)
|   +-- tx/                <- Transaction types: TxMode, TxId (20 LOC)
|   +-- index/             <- Index types: BTree, FullText, Unique, Vector (16 LOC)
|   +-- aiwar.rs           <- AI War + chess knowledge graph (feature: chess)
|   +-- chess.rs           <- Chess procedures (feature: chess)
+-- tests/
|   +-- e2e_basic.rs       <- CREATE, MATCH, WHERE, RETURN (10 tests)
|   +-- e2e_write.rs       <- SET, DELETE, DETACH DELETE (13 tests)
|   +-- e2e_traversal.rs   <- Relationship patterns (10 tests)
|   +-- e2e_aggregation.rs <- COUNT, SUM, GROUP BY, DISTINCT (7 tests)
|   +-- e2e_edge_cases.rs  <- NULL, CASE, type coercion, UNWIND (18 tests)
+-- docs/                  <- ~6,500 lines of documentation
+-- benches/               <- [Future] criterion benchmarks
```

---

## Quick Commands

```bash
# Run all tests (should see 116 passing)
cargo test

# Run with all features
cargo test --all-features

# Run specific test
cargo test test_create_and_get_node

# Fast compile check
cargo check

# Check with all features
cargo check --all-features

# Future: benchmark parser
cargo bench --bench cypher_bench
```

---

*This roadmap is the authoritative integration plan for neo4j-rs and its ecosystem.
All subtasks reference sections by number (e.g., "Implement 4.6 -- expand via edge walk").
Session-safe: contains all context needed to resume work from any point.*
