# neo4j-rs Integration Roadmap

> **Updated**: 2026-02-16 | **Branch**: `claude/add-development-guide-PK55e`
> **Scope**: neo4j-rs + ladybug-rs + holograph trinity
> **Principle**: The StorageBackend trait is the integration seam. All backends
> speak the same contract. All Cypher works identically on every backend.

---

## Phase Overview

```
Phase 1  ██████████████████████░░  90%   Cypher Parser + Memory Backend
Phase 2  █████████████░░░░░░░░░░  55%   Execution Engine Completion
Phase 3  ████░░░░░░░░░░░░░░░░░░  15%   Bolt Protocol Client
Phase 4  ███░░░░░░░░░░░░░░░░░░░  10%   ladybug-rs LadybugBackend
Phase 5  ░░░░░░░░░░░░░░░░░░░░░░   0%   GUI + Developer Tools
Phase 6  ░░░░░░░░░░░░░░░░░░░░░░   0%   Advanced Cypher + TCK
```

---

## Phase 1: Cypher Parser + Memory Backend (CURRENT — 90%)

**Status**: Core pipeline works end-to-end. 116 tests passing.

### Remaining Work

| # | Task | Est. LOC | Priority |
|---|------|:--------:|:--------:|
| 1.1 | Add missing scalar functions (`type()`, `keys()`, `properties()`, `size()`, `coalesce()`) | ~150 | High |
| 1.2 | Add missing aggregation functions (`avg()`, `min()`, `max()`) | ~80 | High |
| 1.3 | Add string functions (`toLower()`, `toUpper()`, `trim()`, `substring()`) | ~60 | Medium |
| 1.4 | Add type conversion functions (`toInteger()`, `toFloat()`, `toString()`) | ~40 | Medium |
| 1.5 | Fix `MERGE` statement planning + execution | ~200 | Medium |
| 1.6 | Implement variable-length path execution (`*1..3`) | ~300 | Medium |
| 1.7 | Add `UNION` / `UNION ALL` execution | ~100 | Low |

### Definition of Done
- All openCypher core clauses parse, plan, and execute on MemoryBackend
- 150+ tests passing
- Zero `todo!()` or `unimplemented!()` in hot path

---

## Phase 2: Execution Engine Completion (55%)

**Goal**: Complete the Volcano-style executor with all Cypher semantics.

### 2A: Real Transactions for MemoryBackend

| # | Task | Est. LOC | Depends On |
|---|------|:--------:|:----------:|
| 2A.1 | Implement write-ahead log (WAL) for MemoryBackend | ~400 | — |
| 2A.2 | Add snapshot isolation (copy-on-write HashMap) | ~300 | 2A.1 |
| 2A.3 | Make `rollback_tx()` actually undo mutations | ~200 | 2A.1 |
| 2A.4 | Add `element_id: Option<String>` to Node/Relationship (Bolt compat) | ~50 | — |
| 2A.5 | Change `ResultRow` from `HashMap` to `IndexMap` (preserve column order) | ~80 | — |

### 2B: Property Indexes

| # | Task | Est. LOC | Depends On |
|---|------|:--------:|:----------:|
| 2B.1 | Implement BTree index for property lookups | ~300 | — |
| 2B.2 | Wire `create_index()` / `drop_index()` to actual index structures | ~100 | 2B.1 |
| 2B.3 | Planner: recognize `WHERE n.prop = val` and use IndexLookup | ~150 | 2B.1, 2B.2 |
| 2B.4 | Unique constraint enforcement on create/set | ~100 | 2B.1 |

### 2C: Cost-Based Optimizer

| # | Task | Est. LOC | Depends On |
|---|------|:--------:|:----------:|
| 2C.1 | Collect basic statistics (label cardinality, property selectivity) | ~200 | — |
| 2C.2 | Cost model: estimate rows per operator | ~300 | 2C.1 |
| 2C.3 | Join ordering: pick optimal Expand sequence | ~200 | 2C.2 |
| 2C.4 | Predicate pushdown: move Filter below Expand when possible | ~150 | 2C.2 |

### Definition of Done
- Transactions have real rollback semantics
- Property indexes accelerate WHERE clause lookups
- Planner chooses index scan vs. full scan based on cardinality

---

## Phase 3: Bolt Protocol Client (15%)

**Goal**: Connect to external Neo4j 5.x as a correctness oracle.

### Architecture
```
neo4j-rs                          External Neo4j 5.x
   │                                    │
   ├── Cypher → AST → Plan             │
   │                                    │
   ├── BoltBackend ──PackStream──────► Bolt port 7687
   │     │                              │
   │     ├── Handshake (Bolt 5.x)       │
   │     ├── HELLO + AUTH               │
   │     ├── RUN + PULL                 │
   │     └── BEGIN/COMMIT/ROLLBACK      │
   │                                    │
   └── Results ◄──PackStream──────────  │
```

### Implementation Tasks

| # | Task | Est. LOC | Depends On | Reference |
|---|------|:--------:|:----------:|-----------|
| 3.1 | PackStream encoder/decoder (serde-based) | ~500 | — | neo4rs `packstream/` |
| 3.2 | Bolt message types (HELLO, RUN, PULL, BEGIN, COMMIT, etc.) | ~400 | 3.1 | neo4rs `bolt/request/` |
| 3.3 | TCP connection + Bolt handshake (version negotiation) | ~300 | 3.1, 3.2 | neo4rs `stream.rs` |
| 3.4 | Authentication (HELLO + credentials) | ~100 | 3.3 | |
| 3.5 | `impl StorageBackend for BoltBackend` — node/rel CRUD | ~600 | 3.3, 3.4 | |
| 3.6 | Transaction management over Bolt (BEGIN/COMMIT/ROLLBACK) | ~200 | 3.5 | |
| 3.7 | Connection pool (`deadpool` or custom) | ~300 | 3.5 | robsdedude driver |
| 3.8 | Causal consistency via bookmarks | ~150 | 3.6 | robsdedude driver |
| 3.9 | Cross-backend test harness (same query → Memory + Bolt → compare) | ~400 | 3.5 | |

### Key Decisions
- **Bolt version**: Target 5.x (Neo4j 5.26.0 from reference repo)
- **Steal from neo4rs**: Their `#[derive(BoltStruct)]` pattern is excellent
- **ValueSend/ValueReceive split**: Users can't send Node objects as parameters
- **Routing**: Read/Write distinction for cluster mode (Phase 3+)

### Definition of Done
- Can connect to Neo4j 5.x, authenticate, and run queries
- All E2E tests pass on both Memory and Bolt backends with identical results
- Connection pooling with configurable limits

---

## Phase 4: ladybug-rs LadybugBackend (10%)

**Goal**: Implement `StorageBackend` for ladybug-rs with CAM-addressed containers.

### Architecture
```
neo4j-rs Cypher Query
   │
   ├── Parse → Plan → Execute
   │
   └── LadybugBackend
         │
         ├── NodeId(u64) ↔ PackedDn (7-level hierarchy)
         │
         ├── create_node()
         │     └── Fingerprint from labels + properties
         │     └── Write to CAM: type_ns(label) | fingerprint_prefix
         │     └── CogRecord: W0=PackedDn, W40-47=label Bloom, Content=property FP
         │
         ├── expand()
         │     └── Inline edge walk: W16-31 (64 max direct)
         │     └── CSR overflow: W96-111 (12 max overflow)
         │     └── Bloom pre-filter: W40-47 neighbor membership
         │
         ├── MATCH → scan(type_id, prefix) on Arrow buffers
         │     └── L1 scent index (1.25 KB, ~50ns): 98.8% elimination
         │     └── L2 scent index (320 KB): 99.997% elimination
         │     └── SIMD Hamming on remaining candidates
         │
         └── vector_query() → CAKES k-NN
               └── Belichtungsmesser 7-point sampling (~14 cycles)
               └── Full SIMD Hamming on survivors
               └── O(k · 2^LFD · log(n)) sublinear guarantee
```

### Implementation Tasks

| # | Task | Est. LOC | Depends On | Location |
|---|------|:--------:|:----------:|----------|
| 4.1 | `NodeId ↔ PackedDn` address translation | ~100 | — | `src/storage/ladybug.rs` |
| 4.2 | `fingerprint_from_node()` — deterministic FP from labels + props | ~200 | ladybug-rs contract | `src/storage/ladybug.rs` |
| 4.3 | `verb_from_rel_type()` — map rel types to 144 Go verbs + hash fallback | ~100 | ladybug-rs contract | `src/storage/ladybug.rs` |
| 4.4 | `impl StorageBackend for LadybugBackend` — node CRUD | ~300 | 4.1, 4.2 | `src/storage/ladybug.rs` |
| 4.5 | Relationship CRUD via inline edges (W16-31) + CSR overflow | ~200 | 4.3, 4.4 | `src/storage/ladybug.rs` |
| 4.6 | `expand()` via edge walk + Bloom pre-filter | ~200 | 4.5 | `src/storage/ladybug.rs` |
| 4.7 | `vector_query()` via CAKES k-NN + Hamming | ~150 | ladybug-rs CAKES | `src/storage/ladybug.rs` |
| 4.8 | `call_procedure()` routing to cognitive modules | ~200 | ladybug-rs cognitive | `src/storage/ladybug.rs` |
| 4.9 | Cross-backend test harness (Memory vs. Ladybug) | ~400 | 4.4-4.8 | `tests/` |

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
| `db.index.vector.queryNodes` | Neo4j 5.x standard | Routes to CAKES |

### Translation Contract

| Neo4j Concept | Ladybug Equivalent | Container Location |
|---------------|-------------------|-------------------|
| `Node` with labels + properties | 8192-bit CogRecord | 1 KB metadata + 1 KB content |
| `NodeId(u64)` | `PackedDn` (7-level hierarchy) | W0 |
| `Relationship` type | One of 144 verbs or hash-fallback | W16-31 inline edges |
| Property access | Metadata field or content FP | Word-level read |
| Label membership | Bloom filter | W40-47 |
| Graph traversal | Inline edge walk + Bloom | W16-31 + W40-47 |
| Vector similarity | CAKES k-NN + Hamming | Content container |
| NARS truth values | Frequency + confidence | W4-7 |

### Definition of Done
- All E2E tests pass on Memory, Bolt, AND Ladybug backends
- `vector_query()` returns real CAKES k-NN results
- Cypher `MATCH` uses scent index for sub-millisecond label scans
- No ladybug-rs internal types leak into neo4j-rs core

---

## Phase 5: GUI + Developer Tools (0%)

**Goal**: Provide visual graph exploration and query tooling.

### 5A: Web-Based Graph Explorer (Primary Recommendation)

```
┌──────────────────────────────────────────────────┐
│  neo4j-rs Web GUI                                │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │  Cypher Editor (CodeMirror + syntax hl)  │   │
│  │  > MATCH (n:Person)-[:KNOWS]->(m)        │   │
│  │  > RETURN n, m                           │   │
│  └──────────────────────────────────────────┘   │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │        Force-directed graph viz           │   │
│  │     ○ Alice ──KNOWS──> ○ Bob             │   │
│  │         \                 |               │   │
│  │          KNOWS           KNOWS            │   │
│  │           \               |               │   │
│  │            ○ Carol ──────>○ Dave          │   │
│  └──────────────────────────────────────────┘   │
│                                                  │
│  Properties: {name: "Alice", age: 30}           │
│  Labels: [:Person, :Employee]                    │
│  Relationships: 2 outgoing, 1 incoming          │
└──────────────────────────────────────────────────┘
```

| # | Task | Technology | Est. LOC |
|---|------|-----------|:--------:|
| 5A.1 | HTTP API server (axum) with JSON endpoints | Rust + axum | ~500 |
| 5A.2 | `/api/query` endpoint (POST Cypher → JSON result) | axum | ~100 |
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
- Alternative: Embed `egui` for native desktop GUI (see 5B below)

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

---

## Phase 6: Advanced Cypher + TCK Compliance (0%)

**Goal**: Full openCypher compatibility validated against the official TCK.

| # | Task | Est. LOC | Priority |
|---|------|:--------:|:--------:|
| 6.1 | Run openCypher TCK test suite (from reference Neo4j repo) | ~200 harness | High |
| 6.2 | Implement `shortestPath()` / `allShortestPaths()` | ~400 | High |
| 6.3 | Implement `FOREACH` clause | ~150 | Medium |
| 6.4 | Implement `LOAD CSV` | ~300 | Medium |
| 6.5 | Implement `CALL { subquery }` (correlated subquery) | ~300 | Medium |
| 6.6 | Implement list comprehensions `[x IN list WHERE pred \| expr]` | ~200 | Medium |
| 6.7 | Implement pattern comprehensions `[(a)-->(b) \| b.name]` | ~200 | Medium |
| 6.8 | Implement map projections `n { .name, .age, score: 100 }` | ~100 | Low |
| 6.9 | Full graph algorithms library (separate crate) | ~2000 | Low |

---

## Dependency Graph

```
Phase 1 (Parser + Memory)
   │
   ├────────────┬──────────────┐
   │            │              │
   v            v              v
Phase 2      Phase 3        Phase 5
(Executor)   (Bolt)         (GUI)
   │            │
   │            v
   │      Cross-backend
   │       test harness
   │            │
   └─────┬──────┘
         │
         v
      Phase 4
    (Ladybug)
         │
         v
      Phase 6
    (TCK + Advanced)
```

**Critical Path**: Phase 1 → Phase 2A (transactions) → Phase 3 (Bolt) → Phase 4 (Ladybug)

**Parallel Work** (can proceed independently):
- Phase 5 (GUI) can start now — only needs the existing `graph.execute()` API
- Phase 2B (indexes) and Phase 2C (optimizer) are independent of Bolt/Ladybug
- Phase 6 TCK tests can start running against Memory backend immediately

---

## Timeline Estimates

| Phase | Scope | Effort | Can Start |
|-------|-------|:------:|-----------|
| Phase 1 remainder | Missing functions + MERGE | 2-3 days | Now |
| Phase 2A: Transactions | WAL + snapshot isolation | 1 week | Now |
| Phase 2B: Indexes | BTree + planner integration | 1 week | Now |
| Phase 2C: Optimizer | Cost model + join ordering | 1-2 weeks | After 2B |
| Phase 3: Bolt | Full PackStream + client | 2-3 weeks | After Phase 1 |
| Phase 4: Ladybug | StorageBackend impl | 2-3 weeks | After Phase 3 |
| Phase 5A: Web GUI | axum + d3-force frontend | 1-2 weeks | Now |
| Phase 5B: Native GUI | egui desktop app | 1-2 weeks | Now |
| Phase 6: TCK | Full compliance | Ongoing | Now (incremental) |

---

*This roadmap is the authoritative integration plan for neo4j-rs. All subtasks
reference sections by number (e.g., "Implement 4.6 — expand via edge walk").
Session-safe: contains all context needed to resume from any point.*
