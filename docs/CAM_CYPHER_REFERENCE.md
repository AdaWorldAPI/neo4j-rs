# CAM Cypher Reference — neo4j-rs ↔ ladybug-rs Address Map

> **Source**: `ladybug-rs/src/learning/cam_ops.rs` (4776 lines, 4096 ops)
> **Updated**: 2026-02-16
> **Scope**: Cypher operations live at CAM addresses `0x200`–`0x2FF`
> **Rule**: Every `StorageBackend` trait method routes to a specific CAM op

---

## 1. Integration Architecture

```
┌──────────────────────────────────────────────────┐
│                    GUI (egui/eframe)              │
│  Cypher editor │ Graph viz │ CAM op browser       │
│  Node inspector│ Result table │ Qualia heatmap    │
└───────────────────────┬──────────────────────────┘
                        │
┌───────────────────────▼──────────────────────────┐
│              neo4j-rs  (public API)               │
│                                                   │
│  Graph<B: StorageBackend>                         │
│    .query("MATCH (n:Thought) RETURN n")           │
│    .execute(plan) → QueryResult                   │
│                                                   │
│  Cypher Parser → LogicalPlan → Executor           │
└───────────────────────┬──────────────────────────┘
                        │ StorageBackend trait
┌───────────────────────▼──────────────────────────┐
│         LadybugBackend (new: storage/ladybug.rs)  │
│                                                   │
│  impl StorageBackend for LadybugBackend           │
│    Translates neo4j-rs DTOs ↔ CogRecord/Container │
│    Routes to CAM ops 0x200-0x2FF                  │
│    Delegates to DataFusion for complex queries     │
└───────────────────────┬──────────────────────────┘
                        │
┌───────────────────────▼──────────────────────────┐
│              ladybug-rs internals                  │
│  Container(8192 bit) + CogRecord(2KB)             │
│  DN-Tree + SpineCache + Scent Index               │
│  LanceDB + DataFusion + SIMD Hamming              │
└──────────────────────────────────────────────────┘
```

---

## 2. StorageBackend → CAM Address Routing

Every `StorageBackend` trait method maps to a specific CAM operation:

| StorageBackend Method | CAM Op | Address | ladybug-rs Module |
|----------------------|--------|:-------:|-------------------|
| `create_node()` | CreateNode | `0x220` | `container::graph::insert` + `storage::lance` |
| `get_node()` | MatchNode | `0x200` | `container::graph::get` → CogRecord by PackedDn |
| `delete_node()` | Delete | `0x246` | `container::graph::remove` + `storage::lance` |
| `detach_delete_node()` | DetachDelete | `0x247` | Remove node + all inline/CSR edges |
| `set_property()` | SetProperty | `0x241` | Lance JSON sidecar update + content re-fingerprint |
| `remove_property()` | RemoveProperty | `0x244` | Lance sidecar delete + re-fingerprint |
| `add_label()` | SetLabel | `0x242` | Update W40-47 Bloom filter |
| `remove_label()` | RemoveLabel | `0x245` | Update W40-47 Bloom filter |
| `create_relationship()` | CreateEdge | `0x221` | `container::adjacency` inline (W16-31) or CSR (W96-111) |
| `get_relationship()` | MatchEdge | `0x201` | `container::adjacency::get_edge` |
| `get_relationships()` | MatchEdge | `0x201` | `container::adjacency::edges_for_node(dir)` |
| `expand()` | VariableLength | `0x265` | `container::traversal::container_multi_hop()` + semiring |
| `create_index()` | *(implicit)* | — | Scent index auto-built; Lance ANN on fingerprints |
| `execute_raw()` | *(dispatch)* | — | `query::cypher::CypherParser` → DataFusion SQL |
| `node_count()` | — | — | `graph.records.len()` |
| `vector_query()` | Knn | `0x2C5` | HDR cascade (Belichtungsmesser → L1–L4) |
| `call_procedure()` | *(by name)* | — | Route `ladybug.*` to CAM executor |

---

## 3. Full Cypher CAM Address Map (0x200–0x2FF)

### Pattern Matching (0x200–0x21F)

| Address | Op Name | Cypher Syntax | Notes |
|:-------:|---------|---------------|-------|
| `0x200` | MatchNode | `MATCH (n:Label)` | Scent index → L1/L2 → SIMD scan |
| `0x201` | MatchEdge | `MATCH ()-[r:TYPE]->()` | Inline edge (W16-31) + CSR (W96-111) |
| `0x202` | MatchPath | `MATCH p = (a)-[*]->(b)` | Multi-hop with semiring |
| `0x203` | MatchVariable | `MATCH (n) WHERE n.prop = $val` | Fingerprint prefix match |
| `0x204` | OptionalMatch | `OPTIONAL MATCH` | Outer join semantics |
| `0x205` | MatchSimilar | `MATCH (n) WHERE n.fp SIMILAR TO $q` | **Extension**: Hamming similarity |

### Create / Merge (0x220–0x23F)

| Address | Op Name | Cypher Syntax | Notes |
|:-------:|---------|---------------|-------|
| `0x220` | CreateNode | `CREATE (n:Label {props})` | Insert CogRecord + Lance sidecar |
| `0x221` | CreateEdge | `CREATE (a)-[:TYPE]->(b)` | Inline edge or CSR overflow |
| `0x222` | CreatePath | `CREATE p = (a)-[:T]->(b)-[:T]->(c)` | Batch edge creation |
| `0x223` | Merge | `MERGE (n:Label {key: val})` | Get-or-create semantics |
| `0x224` | MergeOnCreate | `ON CREATE SET n.prop = val` | Conditional on create |
| `0x225` | MergeOnMatch | `ON MATCH SET n.prop = val` | Conditional on match |

### Update (0x240–0x25F)

| Address | Op Name | Cypher Syntax | Notes |
|:-------:|---------|---------------|-------|
| `0x240` | Set | `SET n.prop = val` | General SET dispatch |
| `0x241` | SetProperty | `SET n.prop = val` | Single property in Lance sidecar |
| `0x242` | SetLabel | `SET n:NewLabel` | Update Bloom filter (W40-47) |
| `0x243` | Remove | `REMOVE n.prop` | General REMOVE dispatch |
| `0x244` | RemoveProperty | `REMOVE n.prop` | Delete from Lance sidecar |
| `0x245` | RemoveLabel | `REMOVE n:Label` | Update Bloom filter (W40-47) |
| `0x246` | Delete | `DELETE n` | Remove CogRecord + Lance row |
| `0x247` | DetachDelete | `DETACH DELETE n` | Remove node + all edges |

### Traversal (0x260–0x27F)

| Address | Op Name | Cypher Syntax | Semiring |
|:-------:|---------|---------------|----------|
| `0x260` | ShortestPath | `shortestPath((a)-[*]->(b))` | HammingMinPlus |
| `0x261` | AllShortestPaths | `allShortestPaths(...)` | HammingMinPlus (collect all) |
| `0x262` | AllPaths | All paths between a, b | — |
| `0x263` | BreadthFirst | BFS traversal | BooleanBfs |
| `0x264` | DepthFirst | DFS traversal | — |
| `0x265` | VariableLength | `(a)-[*1..5]->(b)` | `container_multi_hop()` |

### Aggregation (0x280–0x29F)

| Address | Op Name | Cypher Syntax |
|:-------:|---------|---------------|
| `0x280` | Collect | `collect(n)` |
| `0x281` | Count | `count(n)` |
| `0x282` | Sum | `sum(n.prop)` |
| `0x283` | Avg | `avg(n.prop)` |
| `0x284` | Min | `min(n.prop)` |
| `0x285` | Max | `max(n.prop)` |
| `0x286` | PercentileCont | `percentileCont(n.prop, 0.5)` |
| `0x287` | PercentileDisc | `percentileDisc(n.prop, 0.5)` |
| `0x288` | StDev | `stDev(n.prop)` |

### Graph Algorithms (0x2A0–0x2BF)

| Address | Op Name | Notes |
|:-------:|---------|-------|
| `0x2A0` | PageRank | PageRankPropagation semiring |
| `0x2A1` | Betweenness | Betweenness centrality |
| `0x2A2` | Closeness | Closeness centrality |
| `0x2A3` | DegreeCentrality | Inline edge count from W16-31 |
| `0x2A4` | CommunityLouvain | Louvain community detection |
| `0x2A5` | CommunityLabelProp | Label propagation |
| `0x2A6` | WeaklyConnected | BooleanBfs both directions |
| `0x2A7` | StronglyConnected | Tarjan's / Kosaraju's |
| `0x2A8` | TriangleCount | Triangle enumeration |
| `0x2A9` | LocalClustering | Clustering coefficient |

### Similarity — ladybug-rs Extensions (0x2C0–0x2DF)

| Address | Op Name | Notes |
|:-------:|---------|-------|
| `0x2C0` | JaccardSimilarity | Set-based similarity |
| `0x2C1` | CosineSimilarity | Vector cosine |
| `0x2C2` | EuclideanDistance | L2 distance |
| `0x2C3` | OverlapSimilarity | Overlap coefficient |
| `0x2C4` | NodeSimilarity | **Hamming distance** on content containers |
| `0x2C5` | Knn | **HDR cascade**: Belichtungsmesser → L1–L4 |

### Projections (0x2E0–0x2FF)

| Address | Op Name | Cypher Syntax |
|:-------:|---------|---------------|
| `0x2E0` | Return | `RETURN expr` |
| `0x2E1` | With | `WITH expr` |
| `0x2E2` | Unwind | `UNWIND list AS item` |
| `0x2E3` | OrderBy | `ORDER BY expr` |
| `0x2E4` | Skip | `SKIP n` |
| `0x2E5` | Limit | `LIMIT n` |
| `0x2E6` | Distinct | `DISTINCT` |
| `0x2E7` | Case | `CASE WHEN ... THEN ... END` |

---

## 4. LadybugBackend Struct

```rust
// Feature-gated: [features] ladybug = ["dep:ladybug-rs"]

use crate::storage::{StorageBackend, Transaction, TxMode, TxId};
use crate::model::*;

pub struct LadybugBackend {
    /// The ladybug-rs ContainerGraph (owns DN-Tree + SpineCache)
    graph: ladybug_rs::container::ContainerGraph,
    /// DataFusion session for complex queries
    df_ctx: ladybug_rs::query::datafusion::LadybugSessionContext,
    /// CAM executor for operation dispatch
    cam: ladybug_rs::learning::cam_ops::CamExecutor,
    /// LanceDB connection for persistence
    lance: ladybug_rs::storage::lance::LanceStore,
    /// ID counter (NodeId = u64, maps to PackedDn internally)
    next_id: AtomicU64,
    /// Bidirectional map: NodeId ↔ PackedDn
    id_map: RwLock<BiMap<u64, PackedDn>>,
}
```

---

## 5. DTO Translation Layer

### Node ↔ CogRecord

```rust
impl LadybugBackend {
    /// neo4j-rs Node → CogRecord + Container
    fn node_to_cogrecord(&self, node: &Node) -> CogRecord {
        let mut record = CogRecord::new();
        // Meta container (W0): PackedDn from id_map
        let dn = self.id_map.read().get_by_left(&node.id.0).copied()
            .unwrap_or_else(|| self.allocate_dn(&node.labels));
        record.meta_mut().set_dn(dn);
        // Meta W3: label hash
        record.meta_mut().set_label_hash(hash_labels(&node.labels));
        // Content container: fingerprint from properties
        let content_fp = properties_to_fingerprint(&node.properties);
        record.set_content(content_fp);
        // Properties stored as JSON in Lance sidecar
        record
    }

    /// CogRecord → neo4j-rs Node
    fn cogrecord_to_node(&self, record: &CogRecord) -> Node {
        let dn = record.meta().dn();
        let id = self.id_map.read().get_by_right(&dn).copied()
            .unwrap_or(0);
        Node {
            id: NodeId(id),
            labels: self.labels_from_hash(record.meta().label_hash()),
            properties: self.load_properties(dn), // from Lance JSON sidecar
        }
    }
}
```

### Relationship ↔ Inline Edge / CSR

Relationships in neo4j-rs are full objects (`Relationship { id, src, dst, rel_type, properties }`).
In ladybug-rs, edges are:

| Storage | Location | Format | Capacity |
|---------|----------|--------|:--------:|
| Inline edges | W16-31 | `verb:u8 \| target_hint:u8` | 64 max |
| CSR overflow | W96-111 | `verb_id:u16 \| weight_q:u16 \| target_dn:u32` | 12 max |

**Translation strategy**:
1. `rel_type` → verb ID via `CamExecutor::verb_to_id()` (namespace `0x0100–0x01FF`)
2. Store in inline edges first; overflow to CSR when >64 edges
3. Properties stored in Lance edge sidecar table (keyed by `{src_dn}_{verb}_{dst_dn}`)
4. `get_relationships(dir=OUTGOING)`: scan W16-31 + W96-111 of source node
5. `get_relationships(dir=INCOMING)`: reverse lookup via `graph.children` map or Lance edge table

---

## 6. Traversal: expand() → Semiring Selection

```rust
async fn expand(
    &self, tx: &Self::Tx, node: NodeId, dir: Direction,
    rel_types: &[&str], depth: ExpandDepth,
) -> Result<Vec<Path>> {
    let dn = self.resolve_dn(node)?;

    // Choose semiring based on query shape:
    // - Simple reachability:  BooleanBfs
    // - Shortest path:        HammingMinPlus
    // - Similarity search:    ResonanceSearch
    // - Path composition:     HdrPathBind
    let semiring = self.pick_semiring(rel_types, depth);

    let verb_ids: Vec<u8> = rel_types.iter()
        .map(|rt| self.verb_to_id(rt))
        .collect();

    let results = container_multi_hop(
        &self.graph,
        dn,
        &verb_ids,
        depth.max_hops(),
        &semiring,
    );

    // Convert DN paths → neo4j-rs Path objects
    results.iter().map(|r| self.dn_path_to_path(r)).collect()
}
```

---

## 7. Complex Cypher via DataFusion

For queries the executor can't handle with simple trait calls (aggregations,
subqueries, OPTIONAL MATCH), route through ladybug-rs's existing Cypher transpiler:

```rust
async fn execute_raw(
    &self, tx: &Self::Tx, query: &str, params: ParamMap,
) -> Result<QueryResult> {
    // ladybug-rs already has: query/cypher.rs → CypherParser → SQL CTEs
    let cypher_query = ladybug_rs::query::cypher::parse(query)?;
    let sql = ladybug_rs::query::cypher::transpile_to_sql(&cypher_query, &params)?;

    // Execute via DataFusion with ladybug UDFs registered
    let df_result = self.df_ctx.sql(&sql).await?;

    // Convert Arrow RecordBatch → neo4j-rs QueryResult
    arrow_to_query_result(df_result)
}
```

---

## 8. Container Metadata Word Layout

Reference: `ladybug-rs/src/container/meta.rs` — 128 × u64 words

```
Word Range   Purpose                     neo4j-rs Mapping
─────────────────────────────────────────────────────────────────
W0           PackedDn (7-level address)  Node.id (via BiMap)
W1           node_kind, flags            Label type hint
W2-3         Reserved / label hash       Label Bloom seed
W4-7         NARS truth values           (ladybug-only: freq, conf)
W8           Gate state                  (ladybug-only: FLOW/HOLD/BLOCK)
W9-11        Reserved
W12-15       Layer markers (10 layers)   (ladybug-only: thinking_style)
W16-31       Inline edges (64 max)       Relationship storage (primary)
W32-39       Q-values (16 × f32)         (ladybug-only: RL routing)
W40-47       Bloom filter (64-bit)       Node.labels membership test
W48-55       Graph metrics               (future: pagerank, degree)
W56-63       Qualia channels (18 × f16)  (ladybug-only: affect)
W64-95       Reserved
W96-111      CSR overflow edges (12 max) Relationship storage (overflow)
W112-127     Reserved
```

Content Container (1 KB = 8192 bits):
- Stores the **fingerprint** derived from node properties
- Hamming distance between containers = semantic similarity
- Expected distance between random containers: 4096 bits (σ ≈ 45)

---

## 9. Key ladybug-rs Source Files

| Purpose | File | Notes |
|---------|------|-------|
| StorageBackend trait | `neo4j-rs/src/storage/mod.rs` | The sacred contract |
| MemoryBackend (oracle) | `neo4j-rs/src/storage/memory.rs` | Reference impl |
| Model DTOs | `neo4j-rs/src/model/` | Node, Relationship, Value, Path |
| Cypher parser (neo4j-rs) | `neo4j-rs/src/cypher/` | lexer.rs, parser.rs, ast.rs |
| Cypher transpiler (ladybug) | `ladybug-rs/src/query/cypher.rs` | Cypher → SQL CTEs |
| **CAM ops (all 4096)** | `ladybug-rs/src/learning/cam_ops.rs` | **4776 lines** |
| CogRecord / Container | `ladybug-rs/src/container/record.rs` | 2 KB atomic unit |
| Fingerprint | `ladybug-rs/src/core/fingerprint.rs` | 8192-bit containers |
| DN-Tree / PackedDn | `ladybug-rs/src/container/graph.rs` | Hierarchical addressing |
| Meta layout (W0-W127) | `ladybug-rs/src/container/meta.rs` | 128 × u64 words |
| Adjacency (inline + CSR) | `ladybug-rs/src/container/adjacency.rs` | W16-31 + W96-111 |
| SpineCache | `ladybug-rs/src/container/spine.rs` | Lock-free XOR summaries |
| Semiring traversal | `ladybug-rs/src/container/semiring.rs` | BooleanBfs, HammingMinPlus, etc. |
| DataFusion bridge | `ladybug-rs/src/query/datafusion.rs` | SQL execution |
| Lance storage | `ladybug-rs/src/storage/lance.rs` | Columnar persistence |
| Scent index | `ladybug-rs/src/core/scent.rs` | L1 (1.25 KB) + L2 (320 KB) |
| Qualia stack | `ladybug-rs/src/qualia/` | 7 layers |

---

## 10. Critical Rules

1. **NEVER import ladybug types into neo4j-rs core** — only in `storage/ladybug.rs` behind feature gate
2. **NEVER import holograph directly** — ladybug-rs owns that dependency
3. **MemoryBackend is the test oracle** — every LadybugBackend behavior must match
4. **Properties go in Lance sidecar** — CogRecord stores fingerprints + metadata, NOT arbitrary JSON
5. **PackedDn is the real identity** — `NodeId(u64)` is a neo4j-rs-facing alias; the BiMap bridges them
6. **Edge type → verb ID mapping** — `rel_type` strings map to `0x0100–0x01FF` namespace
7. **Traversal → semiring selection** — pick the right semiring for the query pattern
8. **CAM dispatch for CALL procedures** — `CALL ladybug.knn(...)` routes to `0x2C5`
9. **GUI is a separate binary crate** — depends on neo4j-rs, never the reverse
10. **Arrow RecordBatch is the result wire format** — for DataFusion interop

---

*This document is the authoritative CAM ↔ Cypher address reference. All
`LadybugBackend` implementations MUST route through the addresses listed here.
Cross-reference with `ladybug-rs/src/learning/cam_ops.rs` for the full 4096-op catalog.*
