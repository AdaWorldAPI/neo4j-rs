# Integration Plan — neo4j-rs as Cypher Front-End to BindSpace

> **Date**: 2026-02-16 (rev 2 — corrected after reading BindSpace source)
>
> **Correction**: Rev 1 proposed duplicating ladybug types in neo4j-rs.
> Wrong. BindSpace IS the universal DTO. LadybugBackend wraps a BindSpace
> instance directly. No translation layer needed — just mapping.
>
> **Vision**: neo4j-rs becomes the Cypher front-end to the entire ladybug
> cognitive architecture. Standard Cypher gives you graph CRUD. CALL
> procedures give you NARS inference, causal reasoning, XOR-fold spine
> queries, consciousness rung classification, and agent orchestration.

---

## 0. The Correction

Rev 1 got this wrong:
- Proposed `ladybug-contract` as dependency → **Wrong.** Need `ladybug` (main crate)
  because `BindSpace`, `PackedDn`, `SpineCache` live there, not in the contract crate.
- Proposed `NodeTranslator`, `VerbTable`, `PropertyFingerprinter` as new types →
  **Wrong.** BindSpace already has `verb("CAUSES")`, `label_fingerprint()`,
  `dn_path_to_addr()`, `write_dn_path()`. No need to duplicate.
- Proposed `BTreeMap<u64, CogRecord>` as storage → **Wrong.** BindSpace IS the
  storage. 65,536 addressable slots at 3-5 cycles per lookup via `Addr(prefix:slot)`.
- Proposed separate edge storage → **Wrong.** BindSpace has `link()`, `edges_out()`,
  `edges_in()`, `traverse()`, `traverse_n_hops()`, plus `BitpackedCsr` for zero-copy.

What BindSpace actually provides (already implemented, 2087 lines):

```
BindSpace.write(fingerprint)           → Addr        // CREATE
BindSpace.read(addr)                   → &BindNode   // MATCH
BindSpace.link(from, verb, to)         → usize       // CREATE relationship
BindSpace.traverse(from, verb)         → Vec<Addr>   // MATCH ()-[r:VERB]->()
BindSpace.traverse_n_hops(a, v, n)     → Vec<(hop, Addr)>  // variable-length path
BindSpace.edges_out(addr)              → Iterator<&BindEdge>
BindSpace.edges_in(addr)               → Iterator<&BindEdge>
BindSpace.verb("CAUSES")              → Option<Addr>  // Surface 0x07 lookup
BindSpace.meta(addr)                   → MetaView     // NARS, edges, rung, qualia
BindSpace.content(addr)                → Container    // search fingerprint
BindSpace.nars_revise(addr, f, c)      → ()          // in-place NARS revision
BindSpace.write_dn_path(path, fp, rung) → Addr       // hierarchical create
BindSpace.resolve("bindspace://path")  → Option<Addr>
BindSpace.parent(addr)                 → Option<Addr> // O(1) tree navigation
BindSpace.ancestors(addr)              → Iterator<Addr>
BindSpace.siblings(addr)               → Iterator<Addr>
BindSpace.children_raw(addr)           → &[u16]       // zero-copy CSR
BindSpace.rebuild_csr()                → ()           // build BitpackedCsr
BindSpace.dirty_addrs()                → Iterator<Addr>
BindSpace.hash_all()                   → [u64; 256]   // XOR-fold integrity
```

**BindSpace already has a QueryAdapter trait:**
```rust
pub trait QueryAdapter {
    fn execute(&self, space: &mut BindSpace, query: &str) -> QueryResult;
}
```

neo4j-rs's Cypher execution engine becomes a `QueryAdapter` implementation.

---

## 1. What LadybugBackend Actually Is

```rust
// neo4j-rs/src/storage/ladybug/mod.rs

use ladybug::storage::bind_space::{BindSpace, Addr, BindNode, FINGERPRINT_WORDS};
use ladybug::container::{Container, MetaView, MetaViewMut};
use ladybug::container::adjacency::PackedDn;

pub struct LadybugBackend {
    /// THE storage. Not a wrapper, not a cache — this IS where data lives.
    space: BindSpace,

    /// NodeId(u64) → Addr mapping (neo4j-rs NodeIds are sequential,
    /// BindSpace Addrs are prefix:slot — need a bridge).
    id_to_addr: Vec<Option<Addr>>,        // indexed by NodeId.0
    addr_to_id: HashMap<Addr, NodeId>,

    /// RelId(u64) → edge index in BindSpace.edges
    rel_to_edge: Vec<Option<usize>>,
    edge_to_rel: HashMap<usize, RelId>,

    /// Property storage (BindNode has fingerprint + label + payload,
    /// but Neo4j nodes need full property maps for RETURN projections).
    node_props: HashMap<NodeId, PropertyMap>,
    node_labels: HashMap<NodeId, Vec<String>>,
    rel_props: HashMap<RelId, PropertyMap>,
    rel_types: HashMap<RelId, String>,
    rel_endpoints: HashMap<RelId, (NodeId, NodeId)>,

    /// Counters
    next_node_id: AtomicU64,
    next_rel_id: AtomicU64,
    next_tx_id: AtomicU64,

    /// Label index (same as MemoryBackend — needed for nodes_by_label)
    label_index: HashMap<String, Vec<NodeId>>,
}
```

### Why property storage lives outside BindSpace

BindSpace stores 256-word fingerprints — the XOR-bound content-addressable
representation. But Neo4j `RETURN n.name` needs the original string "Ada",
not bits. Two options:

1. **Payload field**: `BindNode.payload: Option<Vec<u8>>` exists but is
   unstructured. Could store serialized PropertyMap here.
2. **Side HashMap**: Store PropertyMap alongside. Simpler, proven (MemoryBackend
   does this), zero risk of corrupting the fingerprint space.

We use option 2 for correctness. The fingerprint in BindSpace IS the CAM
representation — it's used for similarity search, spine folding, NARS evidence.
The PropertyMap is the human-readable projection for RETURN clauses.

---

## 2. StorageBackend Implementation — Method by Method

### 2.1 Node CRUD

```rust
async fn create_node(&self, tx: &mut Tx, labels: &[&str], props: PropertyMap) -> Result<NodeId> {
    let id = NodeId(self.next_node_id.fetch_add(1, Ordering::Relaxed));

    // 1. Fingerprint properties into 256-word vector
    let fp = fingerprint_properties(&props);

    // 2. Write to BindSpace (allocates in node zone 0x80-0xFF)
    let label_str = labels.join(":");
    let addr = self.space.write_labeled(fp, &label_str);

    // 3. Set NARS initial truth in metadata
    //    New node: freq=0.5, conf=0.01 (uncertain, no evidence yet)
    self.space.nars_revise(addr, 0.5, 0.01);

    // 4. Register bidirectional ID ↔ Addr mapping
    self.id_to_addr[id.0 as usize] = Some(addr);
    self.addr_to_id.insert(addr, id);

    // 5. Store properties and labels (for RETURN projections)
    self.node_props.insert(id, props);
    self.node_labels.insert(id, labels.iter().map(|l| l.to_string()).collect());

    // 6. Update label index
    for label in labels {
        self.label_index.entry(label.to_string()).or_default().push(id);
    }

    Ok(id)
}

async fn get_node(&self, tx: &Tx, id: NodeId) -> Result<Option<Node>> {
    let addr = self.id_to_addr[id.0 as usize].ok_or(NotFound)?;

    // Read BindNode (O(1) array lookup, 3-5 cycles)
    let _bind_node = self.space.read(addr).ok_or(NotFound)?;

    // Reconstruct Neo4j Node from stored properties
    Ok(Some(Node {
        id,
        element_id: None,
        labels: self.node_labels.get(&id).cloned().unwrap_or_default(),
        properties: self.node_props.get(&id).cloned().unwrap_or_default(),
    }))
}
```

### 2.2 Relationship CRUD

```rust
async fn create_relationship(
    &self, tx: &mut Tx, src: NodeId, dst: NodeId,
    rel_type: &str, props: PropertyMap,
) -> Result<RelId> {
    let id = RelId(self.next_rel_id.fetch_add(1, Ordering::Relaxed));

    let src_addr = self.id_to_addr[src.0 as usize].ok_or(NotFound)?;
    let dst_addr = self.id_to_addr[dst.0 as usize].ok_or(NotFound)?;

    // 1. Resolve verb from Surface 0x07
    //    CAUSES → Addr(0x07, 0x00), BECOMES → Addr(0x07, 0x01), etc.
    //    For unknown verbs: register dynamically in the verb surface.
    let verb_addr = self.resolve_verb(rel_type);

    // 2. Link in BindSpace (creates BindEdge with XOR-bound fingerprint:
    //    edge.fp = src.fp ⊕ verb.fp ⊕ dst.fp)
    let edge_idx = self.space.link(src_addr, verb_addr, dst_addr);

    // 3. Register mapping
    self.rel_to_edge[id.0 as usize] = Some(edge_idx);
    self.edge_to_rel.insert(edge_idx, id);

    // 4. Store properties, type, endpoints
    self.rel_props.insert(id, props);
    self.rel_types.insert(id, rel_type.to_string());
    self.rel_endpoints.insert(id, (src, dst));

    // 5. NARS: this relationship is evidence.
    //    Revise source node's truth value toward higher confidence.
    self.space.nars_revise(src_addr, 0.7, 0.3);

    Ok(id)
}

fn resolve_verb(&mut self, rel_type: &str) -> Addr {
    // 1. Try exact match in Surface 0x07 (CAUSES, BECOMES, KNOWS, etc.)
    if let Some(addr) = self.space.verb(rel_type) {
        return addr;
    }

    // 2. Try normalized (lowercase "invests in" → "INVESTS_IN")
    let normalized = rel_type.to_uppercase().replace(' ', "_").replace('-', "_");
    if let Some(addr) = self.space.verb(&normalized) {
        return addr;
    }

    // 3. Register new verb in Surface 0x07 (dynamic extension)
    let fp = label_fingerprint(rel_type);
    let addr = self.space.surface_op_or_register(PREFIX_VERBS, rel_type, fp);
    addr
}
```

### 2.3 Traversal

```rust
async fn get_relationships(
    &self, tx: &Tx, node: NodeId, dir: Direction, rel_type: Option<&str>,
) -> Result<Vec<Relationship>> {
    let addr = self.id_to_addr[node.0 as usize].ok_or(NotFound)?;

    let edges: Vec<&BindEdge> = match dir {
        Direction::Outgoing => self.space.edges_out(addr).collect(),
        Direction::Incoming => self.space.edges_in(addr).collect(),
        Direction::Both => {
            let mut v: Vec<_> = self.space.edges_out(addr).collect();
            v.extend(self.space.edges_in(addr));
            v
        }
    };

    // Filter by type and reconstruct Neo4j Relationship objects
    let mut results = Vec::new();
    for edge in edges {
        let edge_idx = /* look up index */;
        if let Some(&rel_id) = self.edge_to_rel.get(&edge_idx) {
            if let Some(rt) = rel_type {
                if self.rel_types.get(&rel_id).map(|s| s.as_str()) != Some(rt) {
                    continue;
                }
            }
            results.push(self.reconstruct_relationship(rel_id));
        }
    }
    Ok(results)
}

async fn expand(
    &self, tx: &Tx, node: NodeId, dir: Direction,
    rel_types: &[&str], depth: ExpandDepth,
) -> Result<Vec<Path>> {
    let addr = self.id_to_addr[node.0 as usize].ok_or(NotFound)?;
    let max_hops = match depth {
        ExpandDepth::Exact(d) => d,
        ExpandDepth::Range { max, .. } => max,
        ExpandDepth::Unbounded => 100,
    };

    // Use BindSpace.traverse_n_hops for BFS expansion
    // This uses the CSR index — zero-copy, no allocation per hop
    if rel_types.len() == 1 {
        if let Some(verb) = self.space.verb(rel_types[0]) {
            let hops = self.space.traverse_n_hops(addr, verb, max_hops);
            return self.hops_to_paths(node, &hops);
        }
    }

    // Multi-type: fall back to per-hop filtering (still uses edges_out)
    self.expand_bfs(addr, node, dir, rel_types, depth).await
}
```

### 2.4 CALL Procedures — The Extension Point

This is where neo4j-rs goes far beyond standard Cypher:

```rust
async fn call_procedure(&self, name: &str, args: &[Value]) -> Result<ProcedureResult> {
    match name {
        // === SEARCH ===
        "ladybug.search" => {
            // Resonance search: fingerprint a query string, find nearest
            // nodes by Hamming distance over the content container.
            // Uses HDR cascade: L0 scent → L1 popcount → L2 sketch → L3 Hamming
            let query = args[0].as_str()?;
            let k = args.get(1).and_then(|v| v.as_int()).unwrap_or(10);
            self.resonance_search(query, k)
        }

        // === VSA OPERATIONS ===
        "ladybug.bind" => {
            // XOR-bind two fingerprints. Returns the bound result.
            // CALL ladybug.bind("concept_A", "concept_B") YIELD fingerprint
        }
        "ladybug.unbind" => {
            // Same as bind (XOR is self-inverse).
            // Given edge ⊕ known ⊕ verb → recovers unknown.
        }
        "ladybug.similarity" => {
            // Hamming similarity between two nodes or strings.
            // CALL ladybug.similarity("Ada", "OpenAI") YIELD score
        }

        // === NARS INFERENCE ===
        "ladybug.truth" => {
            // Read NARS truth value <freq, conf> from a node's metadata.
            // CALL ladybug.truth(nodeId) YIELD frequency, confidence, expectation
            let addr = self.node_addr(args[0].as_int()?)?;
            let (f, c) = self.space.read(addr).unwrap().nars();
            // Return as procedure result
        }
        "ladybug.revise" => {
            // NARS truth revision: accumulate evidence on a node.
            // CALL ladybug.revise(nodeId, 0.8, 0.6) YIELD frequency, confidence
            let addr = self.node_addr(args[0].as_int()?)?;
            self.space.nars_revise(addr, args[1].as_float()?, args[2].as_float()?);
        }
        "ladybug.deduction" => {
            // NARS deduction: A→B, B→C ⊢ A→C with truth propagation.
            // CALL ladybug.deduction(nodeA, nodeB, nodeC) YIELD frequency, confidence
        }
        "ladybug.abduction" => {
            // NARS abduction: A→B, C→B ⊢ C→A (inference to best explanation).
        }

        // === CAUSAL REASONING (Pearl's Ladder) ===
        "ladybug.rung" => {
            // Classify a node's causal rung: SEE(0), DO(1), IMAGINE(2).
            // CALL ladybug.rung(nodeId) YIELD rung, label
            let addr = self.node_addr(args[0].as_int()?)?;
            let rung = self.space.rung(addr);
        }
        "ladybug.counterfactual" => {
            // Pearl's rung 3: "What if X had been different?"
            // Creates a counterfactual world by cloning subgraph,
            // applying intervention, and propagating consequences.
        }

        // === SPINE / XOR-FOLD ===
        "ladybug.spine" => {
            // XOR-fold all nodes with a given label into a single
            // 256-word digest. Useful for cluster fingerprinting.
            // CALL ladybug.spine("Person") YIELD spine, count, popcount
        }

        // === DN TREE ===
        "ladybug.dn.navigate" => {
            // Navigate the DN tree by path.
            // CALL ladybug.dn.navigate("Ada:A:soul:identity") YIELD addr, depth, rung
            let path = args[0].as_str()?;
            let addr = self.space.write_dn_path(path, ...);
        }
        "ladybug.dn.ancestors" => {
            // Walk up the DN tree from a node.
            // Returns all ancestors as rows.
        }
        "ladybug.dn.children" => {
            // List children of a DN node.
        }

        // === CRYSTALLIZATION ===
        "ladybug.crystallize" => {
            // Mark a belief as frozen (high confidence, won't decay).
            // Sets confidence to 0.99 and TTL to 0 (permanent).
        }

        // === META ===
        "ladybug.capabilities" => {
            // Report what this backend can do.
        }

        _ => Err(Error::ExecutionError(format!("Unknown procedure: {name}")))
    }
}
```

---

## 3. Beyond Standard Cypher — What This Enables

### 3.1 NARS-Augmented Queries

Standard Cypher:
```cypher
MATCH (a:Company)-[:INVESTS_IN]->(b:Company) RETURN a, b
```

Ladybug-augmented:
```cypher
// Find investment relationships with high confidence
MATCH (a:Company)-[r:INVESTS_IN]->(b:Company)
CALL ladybug.truth(a) YIELD confidence AS a_conf
CALL ladybug.truth(b) YIELD confidence AS b_conf
WHERE a_conf > 0.7 AND b_conf > 0.7
RETURN a.name, b.name, a_conf, b_conf
ORDER BY a_conf * b_conf DESC
```

### 3.2 Resonance Search (Semantic Nearest Neighbors)

```cypher
// Find nodes similar to "artificial intelligence regulation"
CALL ladybug.search("artificial intelligence regulation", 10) YIELD nodeId, score
MATCH (n) WHERE id(n) = nodeId
RETURN n.name, score
ORDER BY score DESC
```

### 3.3 Causal Inference

```cypher
// Given: A invests_in B, B develops C
// Infer: A indirectly_supports C (NARS deduction)
MATCH (a)-[:INVESTS_IN]->(b)-[:DEVELOPS]->(c)
CALL ladybug.deduction(a, b, c) YIELD frequency, confidence
WHERE confidence > 0.5
RETURN a.name AS investor, c.name AS technology, frequency, confidence
```

### 3.4 Spine Queries (Cluster Fingerprinting)

```cypher
// XOR-fold all TechCompany nodes into a cluster fingerprint
CALL ladybug.spine("TechCompany") YIELD spine, count, popcount
// Then find nodes similar to the cluster centroid
CALL ladybug.search(spine, 5) YIELD nodeId, score
RETURN nodeId, score
```

### 3.5 Counterfactual Reasoning

```cypher
// Pearl's rung 3: "What if Google hadn't acquired DeepMind?"
MATCH (google:Company {name: "Google"})-[r:ACQUIRES]->(dm:Company {name: "DeepMind"})
CALL ladybug.counterfactual(r) YIELD affected_nodes, belief_delta
RETURN affected_nodes, belief_delta
```

### 3.6 DN Tree Navigation

```cypher
// Navigate the cognitive hierarchy
CALL ladybug.dn.navigate("Ada:A:soul:identity") YIELD addr, depth, rung
// Walk up
CALL ladybug.dn.ancestors(addr) YIELD ancestor_addr, ancestor_depth
MATCH (n) WHERE ladybug.addr(n) = ancestor_addr
RETURN n, ancestor_depth
```

---

## 4. Implementation Sequence

### Phase 1: BindSpace Backend Scaffold

Create `src/storage/ladybug/mod.rs` with:
- `LadybugBackend` struct wrapping `BindSpace`
- Bidirectional `NodeId ↔ Addr` mapping
- Bidirectional `RelId ↔ edge_index` mapping
- Property/label side storage for RETURN projections
- `impl StorageBackend for LadybugBackend` — all CRUD methods

All standard Cypher operations MUST produce identical results to
MemoryBackend. This is the correctness baseline.

### Phase 2: Verb Resolution

Map Neo4j relationship types to BindSpace Surface 0x07 verbs:
- 28 pre-initialized verbs (CAUSES through PREV_SIBLING)
- Dynamic registration for unknown verbs
- Aiwar pattern: `CONNECTED_TO` + `r.label` property → verb lookup

### Phase 3: Property Fingerprinting

When a node is created:
1. Sort property keys alphabetically
2. XOR-bind each (key, value) pair into a 256-word fingerprint
3. Write fingerprint to BindSpace via `space.write(fp)`
4. Meta container (words 0-127): NARS truth, label_hash, inline edges
5. Content container (words 128-255): semantic fingerprint for search

### Phase 4: CALL Procedures

Register 15+ procedures:
- 4 VSA: search, bind, unbind, similarity
- 4 NARS: truth, revise, deduction, abduction
- 3 DN: navigate, ancestors, children
- 2 Causal: rung, counterfactual
- 1 Spine: xor-fold
- 1 Crystal: crystallize

### Phase 5: Acceptance Test with aiwar_full.cypher

- 221 nodes create with correct fingerprints
- 356 relationships create with correct verb bindings
- All standard Cypher queries match MemoryBackend results
- NARS truth values accumulate from relationship evidence
- `ladybug.search()` returns resonance-ranked results
- Spine queries produce correct XOR-folds per label

### Phase 6: NARS Calibration Pipeline

After loading aiwar data:
1. For each verb type, count evidence (pos/neg)
2. Initialize `TruthValue::from_evidence(pos, neg)` per verb
3. Run `TruthValue::revision()` to merge multiple evidence sources
4. Run `TruthValue::deduction()` for transitive inferences
5. Crystallize high-confidence beliefs

---

## 5. Dependency Graph (Corrected)

```
neo4j-rs
  └── ladybug (main crate, optional, feature = "ladybug")
        ├── BindSpace              (THE storage — 65K addressable slots)
        │   ├── Addr(prefix:slot)  (O(1) array indexing, 3-5 cycles)
        │   ├── BindNode           (256 u64 fingerprint + label + parent + rung)
        │   ├── BindEdge           (from ⊕ verb ⊕ to, XOR-bound)
        │   ├── BitpackedCsr       (zero-copy edge traversal)
        │   ├── DnIndex            (PackedDn ↔ Addr bidirectional)
        │   └── DirtyBits          (65536-bit change tracking)
        ├── Container              (8192-bit, 128 × u64)
        ├── MetaView / MetaViewMut (zero-copy metadata, 50+ accessors)
        ├── PackedDn               (7-level hierarchy, u64)
        ├── SpineCache             (XOR-fold, lazy recompute)
        └── TruthValue             (NARS, full NAL truth functions)
```

**neo4j-rs depends on ladybug main crate, NOT just ladybug-contract.**
BindSpace, PackedDn, SpineCache are in the main crate.

---

## 6. File-Level Change Map (Corrected)

### In neo4j-rs (NEW files):

| File | Contents | Est. Lines |
|------|----------|:----------:|
| `src/storage/ladybug/mod.rs` | LadybugBackend + StorageBackend impl | ~600 |
| `src/storage/ladybug/procedures.rs` | 15+ CALL procedure handlers | ~400 |
| `src/storage/ladybug/fingerprint.rs` | Property → 256-word fingerprint | ~100 |
| `tests/ladybug_backend.rs` | Acceptance tests (aiwar + NARS) | ~300 |

### Files to REMOVE (premature scaffold from rev 1):

| File | Why |
|------|-----|
| `src/storage/ladybug/translator.rs` | Duplicates PackedDn + DnIndex in BindSpace |
| `src/storage/ladybug/verbs.rs` | Duplicates Surface 0x07 verb table in BindSpace |

### In neo4j-rs (MODIFIED):

| File | Change |
|------|--------|
| `Cargo.toml` | Change dep from `ladybug-contract` to `ladybug` |
| `src/storage/mod.rs` | `pub mod ladybug;` (feature-gated) |

### In ladybug-rs:

| File | Change |
|------|--------|
| **NONE** | **NO CHANGES. Period.** |

---

## 7. What NOT to Duplicate

Now that I've read the code, these are the types I was about to duplicate
and shouldn't have:

| Type I Almost Duplicated | Where It Already Lives | Method |
|--------------------------|----------------------|--------|
| PackedDn | `container/adjacency.rs` | `from_path()`, `parent()`, `child()` |
| VerbTable (144 verbs) | BindSpace Surface 0x07 | `space.verb("CAUSES")` |
| NodeTranslator | BindSpace.DnIndex | `dn_index.register()`, `addr_for()` |
| PropertyFingerprinter | BindSpace | `label_fingerprint()` |
| ContainerDto | `Container` in ladybug-contract | Exact same struct |
| Edge storage | BindSpace.edges + BitpackedCsr | `link()`, `edges_out()` |
| Dirty tracking | BindSpace.DirtyBits | `mark_dirty()`, `dirty_addrs()` |
| NARS revision | BindSpace | `nars_revise(addr, f, c)` |

The only thing neo4j-rs needs to add is the **NodeId ↔ Addr mapping**
(because Neo4j uses sequential u64 IDs while BindSpace uses prefix:slot
addressing) and **property side storage** (because BindSpace stores
fingerprints, not original strings).

---

## 8. Success Criteria

1. `cargo build --features ladybug` compiles
2. All existing MemoryBackend tests pass with LadybugBackend
3. `aiwar_full.cypher` loads: 221 nodes, 356 relationships
4. `CALL ladybug.search("AI regulation", 10)` returns ranked results
5. `CALL ladybug.truth(nodeId)` returns NARS freq/conf
6. `CALL ladybug.spine("TechCompany")` returns XOR-fold
7. Zero changes to ladybug-rs
8. BindSpace.stats() shows correct surface/fluid/node counts

---

---

## 9. SPO/XYZ Holographic Geometry (Rev 3)

> Added after reading `extensions/spo/spo.rs` (1571 lines),
> `container/traversal.rs` (440 lines), `container/search.rs` (247 lines),
> `qualia/felt_traversal.rs` (180+ lines), and the blasgraph lineage
> (ARCHITECTURE.md Section 26).

### The Insight

The DN-Tree IS the sparse adjacency already. Each CogRecord's W16-31
(64 inline edges) + W96-111 (CSR overflow) is the RedisGraph CSR layout
transcoded to cognitive verb IDs + Container target hints:

```text
RedisGraph (CSR integer IDs)
  → Holograph (DnNodeStore + DnCsr, fingerprint IDs)
    → BlasGraph (sparse adjacent vectors, BLAS-style ops)
      → ContainerGraph (pure Container-native, everything 8192 bits)
```

### SPO Triple Encoding

Every relationship in LadybugBackend now carries a **holographic SPO trace**:

```text
S = Subject container   (8192 bits) — node fingerprint
P = Predicate container (8192 bits) — verb fingerprint (e.g., "CAUSES")
O = Object container    (8192 bits) — node fingerprint

trace = permute(S,1) ⊕ ROLE_S ⊕ permute(P,43) ⊕ ROLE_P ⊕ permute(O,89) ⊕ ROLE_O
```

**Permutation** (word-level circular shift) breaks XOR commutativity:
`A→B` and `B→A` produce different traces because `permute(A,1) ≠ permute(A,89)`.
This is the standard VSA approach — ladybug's SPO Crystal uses orthogonal
codebooks for the same purpose; we use permutation since we don't maintain
a codebook in the bridge layer.

### Holographic Recovery

Given any 2 components + trace, recover the 3rd via pure XOR:

```text
MATCH (s)-[:CAUSES]->(?)     →  missing_O = recover_object(trace, S, P)
MATCH (?)-[:CAUSES]->(o)     →  missing_S = recover_subject(trace, P, O)
MATCH (s)-[?]->(o)           →  missing_P = recover_predicate(trace, S, O)
```

No index lookup needed. ~256 XOR + rotate operations (128 words × 2 ops).

### Belichtungsmesser HDR Cascade

Similarity search now uses the 3-level cascade from `container/search.rs`:

| Level | Operation | Cycles | Purpose |
|-------|-----------|--------|---------|
| L0 | Belichtungsmesser (7 samples) | ~14 | Rejects ~90% of candidates |
| L1 | Exact Hamming with early exit | ~128 | Prunes distant survivors |
| L2 | Full ranking | ~256 | Final top-k |

This replaces the previous O(n × 8192-bit) linear scan with O(n × 448-bit)
pre-filter + O(0.1n × 8192-bit) exact pass.

### Semiring Traversal

Five concrete semirings mirror `container/traversal.rs`:

| Semiring | Value | Use Case |
|----------|-------|----------|
| BooleanBfs | `bool` | Reachability queries (MATCH path exists) |
| HammingMinPlus | `u32` | Shortest semantic path |
| HdrPathBind | `Option<ContainerDto>` | XOR-compose path fingerprints |
| ResonanceSearch | `u32` | Find paths resonating with query |
| CascadedHamming | `u32` | Belichtungsmesser-accelerated shortest path |

All operate on `ContainerDto` (neo4j-rs local type) and will bridge to
ladybug's `DnSemiring` over `Container` when compiled together.

### New Procedures

| Procedure | Args | Operation |
|-----------|------|-----------|
| `ladybug.spo.trace(s, p, o)` | 3 strings | Compute holographic trace + verify recovery |
| `ladybug.spo.recover(k1, k2, trace, role)` | 3 strings + role | Recover missing SPO component |
| `ladybug.abduction(f1, c1, f2, c2)` | NARS truth values | A→B, B ⊢ A (weak inference) |
| `ladybug.induction(f1, c1, f2, c2)` | NARS truth values | A, A→B ⊢ generalise |

### Integration Chain

```text
Cypher: MATCH (a)-[:CAUSES]->(b)
  → neo4j-rs parses, plans expand()
    → LadybugBackend dispatches via SPO trace
      → For known-subject queries: recover_object(trace, S, P)
      → Belichtungsmesser cascade finds closest node to recovered fingerprint
        → ~14 cycles per candidate, 90% rejection at L0
      → For multi-hop: semiring MxV over adjacency (container_mxv)
        → Each hop reads InlineEdges from CogRecord W16-31
        → DnSemiring multiply + add per edge
```

### File Map (Rev 3)

| File | Lines | Purpose |
|------|-------|---------|
| `src/storage/ladybug/mod.rs` | ~500 | LadybugBackend + StorageBackend impl + SPO recovery |
| `src/storage/ladybug/fingerprint.rs` | ~360 | ContainerDto + permute/unpermute + PropertyFingerprinter |
| `src/storage/ladybug/spo.rs` | ~350 | SpoTrace + Belichtungsmesser + cascade + semirings |
| `src/storage/ladybug/procedures.rs` | ~350 | 14 CALL procedure handlers |

### What's Left

1. **Bridge to real BindSpace** — Replace RwLock<HashMap> internal storage
   with `BindSpace` instance when `#[cfg(feature = "ladybug")]` is active.
   The `SpoTrace` already uses the same role vector seeds as ladybug's
   SPO Crystal (0xDEADBEEF_CAFEBABE, etc.).

2. **Felt traversal dispatch** — Wire `CALL ladybug.felt_traverse(dn)` to
   ladybug's `FeltPath` with surprise/free-energy reporting.

3. **GrBMatrix facade** — The `GRAPH.QUERY` command interface wrapping
   `container_mxv()` with XOR semirings. The GrBMatrix rows ARE the
   CogRecord inline edges viewed as sparse matrix format.

4. **Quorum field procedures** — `CALL ladybug.quorum.evolve()` for
   5×5×5 lattice dynamics.

---

*Rev 3: Added SPO/XYZ holographic geometry, Belichtungsmesser cascade,
permutation-based role binding, 5 semiring implementations, and blasgraph
lineage documentation. All tests pass. Zero changes to ladybug-rs.*
