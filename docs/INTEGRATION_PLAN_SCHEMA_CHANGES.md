# Integration Plan — Schema Changes in ladybug-rs for neo4j-rs Bridge

> **Date**: 2026-02-16
> **Context**: The DN tree, containers, and CAM are already solidified in
> `ladybug-contract`. This plan captures what needs to change before the
> architecture knowledge dilutes.
>
> **Principle**: `ladybug-contract` is the source of truth. neo4j-rs depends
> on it, not the other way around.

---

## 0. What's Already Solid (DO NOT CHANGE)

These types are complete, tested, and correct. The integration plan builds
ON them, never modifies them:

| Type | Crate | File | Status |
|------|-------|------|--------|
| `Container` (128 × u64, 8192 bits) | ladybug-contract | `container.rs` | Frozen |
| `CogRecord` (meta + content, 2 KB) | ladybug-contract | `record.rs` | Frozen |
| `ContainerGeometry` (Cam/Xyz/Bridge/Extended/Chunked/Tree) | ladybug-contract | `geometry.rs` | Frozen |
| `MetaView` / `MetaViewMut` (50+ zero-copy accessors) | ladybug-contract | `meta.rs` | Frozen |
| `TruthValue` (f32 freq/conf, full NAL truth functions) | ladybug-contract | `nars.rs` | Frozen |
| `CogPacket` (binary wire: 64B header + 1-2 containers) | ladybug-contract | `wire.rs` | Frozen |
| `CognitiveAddress` (64-bit domain/subtype) | ladybug-contract | `address.rs` | Frozen |
| `OpCategory` / `OpSignature` (CAM codebook) | ladybug-contract | `codebook.rs` | Frozen |
| `PackedDn` (7-level hierarchy, u64) | ladybug (main) | `container/adjacency.rs` | Frozen |
| `SpineCache` (XOR-fold, lock-free) | ladybug (main) | `container/spine.rs` | Frozen |
| `SchemaSidecar` (32-word, 16K compat) | ladybug (main) | `width_16k/schema.rs` | Frozen |
| `BindSpace` / `Addr` (8+8 addressing) | ladybug (main) | `storage/bind_space.rs` | Frozen |

**The metadata layout (W0-W127 in meta.rs) is the master schema.** Everything
else derives from it. The 8192-bit metadata container already has:
- W0: PackedDn identity
- W1: node_kind | container_count | geometry | flags | schema_version
- W4-7: NARS (freq, conf, pos_evidence, neg_evidence)
- W16-31: Inline edges (64 packed, 4 per word)
- W96-111: DN-sparse adjacency (compact inline CSR)

---

## 1. Bridge DTOs — What neo4j-rs Needs

neo4j-rs has its own type system (NodeId, RelationshipId, Property, Label,
Value). The bridge must translate between these and ladybug-contract types
without either side knowing about the other.

### 1.1 Translation Layer Types (NEW — in neo4j-rs)

These live in neo4j-rs under a new `ladybug` feature gate:

```rust
// neo4j-rs/src/storage/ladybug.rs (new file)

use ladybug_contract::{Container, CogRecord, ContainerGeometry, MetaView, MetaViewMut};

/// Maps a neo4j-rs NodeId(u64) to a ladybug PackedDn address.
/// For string IDs (aiwar pattern), uses deterministic hash.
pub struct NodeTranslator {
    /// Strategy for ID mapping
    strategy: IdStrategy,
    /// Label → verb_mask bit position (144 core + hash fallback)
    verb_table: VerbTable,
}

pub enum IdStrategy {
    /// NodeId(u64) maps directly to PackedDn via level assignment
    Direct,
    /// String ID → u64 via SipHash-2-4 (deterministic, collision-resistant)
    HashFromString,
}

/// Translates Neo4j relationship types + properties to ladybug verb slots.
pub struct VerbTable {
    /// First 144 slots: well-known verbs (CAUSES, BECOMES, PART_OF, etc.)
    core: [Option<&'static str>; 144],
    /// Overflow: SipHash of verb string → slot 144-255
    overflow: HashMap<String, u8>,
}

impl VerbTable {
    /// Extract verb from a relationship.
    /// Configurable: from rel_type, from property, or both.
    pub fn extract_verb(&self, rel_type: &str, properties: &Properties) -> u8 {
        // 1. Check if rel_type is a known verb
        // 2. If CONNECTED_TO, check r.label property (aiwar pattern)
        // 3. Fall back to hash
    }
}
```

### 1.2 Property → Container Fingerprinting (NEW — in neo4j-rs)

```rust
/// Converts Neo4j node properties to a content Container.
///
/// Property keys are sorted alphabetically before fingerprinting
/// (addresses W-3 gotcha from wiring plan: HashMap iteration order).
pub struct PropertyFingerprinter {
    mode: FingerprintMode,
}

pub enum FingerprintMode {
    /// Full record IS fingerprint (CAM standard)
    Cam,
    /// Content container only (bitpacked distance mode)
    Bitpacked,
    /// CAM + external Jina vector (hybrid)
    Hybrid { jina_endpoint: String },
}

impl PropertyFingerprinter {
    /// Convert sorted properties to a Container.
    /// Each key-value pair is hashed and XOR-bound into the container.
    pub fn fingerprint(&self, properties: &[(String, Value)]) -> Container {
        let mut fp = Container::zero();
        let mut sorted: Vec<_> = properties.to_vec();
        sorted.sort_by(|a, b| a.0.cmp(&b.0)); // deterministic order

        for (key, value) in &sorted {
            let key_fp = Container::random(siphash(key));
            let val_fp = Container::random(siphash(&value.to_string()));
            let pair_fp = key_fp.xor(&val_fp);
            fp = fp.xor(&pair_fp);
        }
        fp
    }
}
```

### 1.3 Label → Metadata Binding (NEW — in neo4j-rs)

```rust
/// Binds Neo4j labels into the metadata container.
/// Multi-label nodes (aiwar: Stakeholder:TechCompany:AIDeveloper)
/// XOR-bind ALL labels into a single label fingerprint.
pub fn bind_labels(labels: &[String]) -> u64 {
    let mut hash = 0u64;
    for label in labels {
        hash ^= siphash(label.as_bytes());
    }
    hash
}
```

---

## 2. Schema Changes in ladybug-contract (MINIMAL)

The existing schema is complete. These are the ONLY additions needed:

### 2.1 Add `StorageBackend` Extension Points

The wiring plan identified 5 gaps. These require NEW trait methods in
whatever crate defines the backend trait. They do NOT change ladybug-contract
types — they add methods to the trait that LadybugBackend implements.

In neo4j-rs (`src/storage/mod.rs`), add to `StorageBackend`:

```rust
/// Gap 1: Full-text / vector similarity search.
fn vector_query(
    &self,
    query: Container,        // ladybug-contract type
    k: usize,
    threshold: Option<f32>,
) -> Result<Vec<(NodeId, f32)>> {
    Ok(vec![]) // default: empty (MemoryBackend returns nothing)
}

/// Gap 2: Call a registered procedure.
fn call_procedure(
    &self,
    name: &str,
    args: Vec<Value>,
) -> Result<Vec<Record>> {
    Err(Error::UnsupportedProcedure(name.to_string()))
}

/// Gap 3: Retrieve backend-specific metadata slot.
fn get_metadata_slot(
    &self,
    node_id: NodeId,
    slot: &str,
) -> Result<Option<Value>> {
    Ok(None)
}

/// Gap 4: Batch node creation (performance path).
fn create_nodes_batch(
    &self,
    nodes: Vec<(Vec<String>, HashMap<String, Value>)>,
) -> Result<Vec<NodeId>> {
    // Default: delegate to create_node() one at a time
    nodes.into_iter()
        .map(|(labels, props)| self.create_node(labels, props))
        .collect()
}

/// Gap 5: Report backend capabilities.
fn capabilities(&self) -> BackendCapabilities {
    BackendCapabilities::default() // MemoryBackend: basic only
}
```

```rust
/// What a backend can do beyond basic CRUD.
#[derive(Default)]
pub struct BackendCapabilities {
    pub vector_search: bool,
    pub procedures: Vec<String>,
    pub fingerprint_mode: Option<String>, // "cam", "bitpacked", "hybrid"
    pub batch_create: bool,
    pub tiered_storage: bool,
}
```

### 2.2 No Changes to Container/CogRecord/MetaView

The metadata layout (W0-W127) already has everything needed:
- **W0 (dn_addr)**: Node identity → maps to Neo4j NodeId
- **W3 (label_hash)**: Label binding → maps to Neo4j labels (XOR-bound)
- **W4-7 (NARS)**: Truth values → no Neo4j equivalent (cognitive extension)
- **W16-31 (edges)**: Inline edges → maps to Neo4j relationships
- **W96-111 (adjacency)**: CSR → maps to Neo4j graph topology

No new words, no new fields, no layout changes. The existing 128-word
metadata schema already covers the Neo4j property graph model.

### 2.3 ContainerGeometry — Already Covers All Cases

| Neo4j Pattern | Geometry | Containers | Notes |
|---------------|----------|:----------:|-------|
| Standard node | `Cam` (default) | 1 meta + 1 content = 2 KB | CAM: full record IS fingerprint |
| Node + Jina embedding | `Bridge` | 1 meta + 1 content + external float vector | Hybrid mode |
| 3D spatial node | `Xyz` | 1 meta + 3 content = 4 KB | One container per axis |
| Document node (chunked) | `Chunked` | 1 meta + N content | Summary + chunks |
| Subtree snapshot | `Tree` | 1 meta + N content | BFS heap layout |

The existing 6 geometries cover every case from the roadmap review.
No new geometry variants needed.

---

## 3. Integration Sequence

### Phase A: Dependency Wiring (1 day)

```toml
# neo4j-rs/Cargo.toml
[features]
ladybug = ["ladybug-contract"]

[dependencies]
ladybug-contract = { git = "https://github.com/AdaWorldAPI/ladybug-rs", optional = true }
```

- Add `ladybug-contract` as optional dependency
- Create `src/storage/ladybug.rs` (feature-gated)
- Import `Container`, `CogRecord`, `ContainerGeometry`, `MetaView`

### Phase B: Translation Layer (3 days)

1. **NodeTranslator**: NodeId(u64) ↔ PackedDn mapping
   - Direct mode: NodeId = PackedDn.0 (u64 → u64)
   - Hash mode: String ID → SipHash → u64 (aiwar pattern)

2. **VerbTable**: RelType + properties → verb slot (u8)
   - 144 core verbs (from CAM reference 0x200-0x2FF)
   - Hash fallback for custom verbs
   - Configurable extraction: rel_type, property, or both

3. **PropertyFingerprinter**: Sorted properties → Container
   - Sort keys alphabetically (deterministic)
   - XOR-bind each key-value pair
   - Handle `nan` values (skip, zero, or explicit marker)

4. **LabelBinder**: Multi-label → label_hash (u64)
   - XOR of all label hashes
   - Stored in MetaView W3

### Phase C: LadybugBackend Scaffold (3 days)

```rust
// neo4j-rs/src/storage/ladybug.rs

pub struct LadybugBackend {
    /// Node storage: PackedDn → CogRecord
    records: BTreeMap<u64, CogRecord>,
    /// Translation layer
    translator: NodeTranslator,
    fingerprinter: PropertyFingerprinter,
    verb_table: VerbTable,
    /// Spine cache for XOR-fold navigation
    spine: SpineCache,
}

impl StorageBackend for LadybugBackend {
    // -- CRUD: translate Neo4j ops to CogRecord ops --
    fn create_node(&mut self, labels: Vec<String>, props: HashMap<String, Value>) -> Result<NodeId> {
        let record = CogRecord::new(ContainerGeometry::Cam);
        // 1. Set label_hash in meta via MetaViewMut
        // 2. Fingerprint properties into content container
        // 3. Assign PackedDn address
        // 4. Insert into records + update spine
    }

    fn create_relationship(&mut self, from: NodeId, to: NodeId, rel_type: String, props: HashMap<String, Value>) -> Result<RelationshipId> {
        // 1. Extract verb from rel_type + props (VerbTable)
        // 2. Set inline edge in source node's meta (W16-31)
        // 3. Update adjacency CSR (W96-111)
    }

    // -- Search: fingerprint-accelerated --
    fn vector_query(&self, query: Container, k: usize, threshold: Option<f32>) -> Result<Vec<(NodeId, f32)>> {
        // HdrCascadeExec: L0 scent → L1 popcount → L2 sketch → L3 Hamming → L4 Mexican hat
    }

    // -- Procedures: 10 registered ladybug.* --
    fn call_procedure(&self, name: &str, args: Vec<Value>) -> Result<Vec<Record>> {
        match name {
            "ladybug.search" => { /* resonance search */ }
            "ladybug.bind" => { /* XOR bind two fingerprints */ }
            "ladybug.unbind" => { /* XOR unbind */ }
            "ladybug.similarity" => { /* Hamming similarity */ }
            "ladybug.truth" => { /* NARS truth value */ }
            "ladybug.causality" => { /* NARS causal inference */ }
            "ladybug.crystallize" => { /* freeze belief */ }
            "ladybug.explore" => { /* counterfactual world */ }
            "ladybug.spine" => { /* XOR-fold query */ }
            "ladybug.rung" => { /* Pearl's rung classification */ }
            _ => Err(Error::UnsupportedProcedure(name.into()))
        }
    }
}
```

### Phase D: Acceptance Test (2 days)

Load `aiwar_full.cypher` through LadybugBackend:

1. All 5 constraints create properly
2. All 221 nodes create with correct label hashes
3. All 356 relationships create with correct verb slots
4. `MATCH (n) RETURN count(n)` returns 221
5. `MATCH ()-[r]->() RETURN count(r)` returns 356
6. `MATCH (a)-[:CONNECTED_TO]->(b) WHERE r.label = 'invests in' RETURN a, b` works
7. `CALL ladybug.search(fingerprint, 10)` returns resonance-ranked results
8. Results match MemoryBackend for all standard Cypher queries

**NARS calibration**: After loading, the 356 relationships provide ground-truth
evidence for initializing NARS frequency/confidence values per verb type.

### Phase E: NARS Pipeline Wiring (3 days)

Wire the competitive advantage pipeline:

1. **DN tree navigate**: Use `PackedDn` for O(1) hierarchy traversal
2. **Hamming popcount**: Use `Container::hamming()` (already 128 XOR + 128 popcount)
3. **HDR stacking**: Wire `HdrCascadeExec` from search module (L0→L4)
4. **NARS update**: After each search result, call `TruthValue::revision()` to
   accumulate evidence (frequency + confidence)
5. **Exact causality**: After sufficient evidence, `TruthValue::deduction()` +
   `TruthValue::abduction()` for causal inference

---

## 4. File-Level Change Map

### In neo4j-rs (NEW files):

| File | Contents | Lines (est.) |
|------|----------|:------------:|
| `src/storage/ladybug.rs` | LadybugBackend + StorageBackend impl | ~500 |
| `src/storage/ladybug/translator.rs` | NodeTranslator, VerbTable | ~200 |
| `src/storage/ladybug/fingerprint.rs` | PropertyFingerprinter, LabelBinder | ~150 |
| `src/storage/ladybug/procedures.rs` | 10 ladybug.* procedure handlers | ~300 |
| `tests/ladybug_backend.rs` | Acceptance tests with aiwar_full.cypher | ~200 |

### In neo4j-rs (MODIFIED files):

| File | Change |
|------|--------|
| `Cargo.toml` | Add `ladybug-contract` optional dep |
| `src/storage/mod.rs` | Add 5 gap methods to StorageBackend (all with defaults) |
| `src/storage/mod.rs` | Add `BackendCapabilities` struct |
| `src/storage/mod.rs` | `pub mod ladybug;` (feature-gated) |

### In ladybug-rs (NO changes to contract crate):

| File | Change | Why |
|------|--------|-----|
| None in `ladybug-contract/` | NO CHANGES | Types are complete and frozen |

### In ladybug-rs main crate (OPTIONAL, future):

| File | Change | Why |
|------|--------|-----|
| `src/storage/neo4j_bridge.rs` (new) | HTTP endpoint for neo4j-rs integration | Phase 7A cross-process |
| `src/flight/neo4j_actions.rs` (new) | Arrow Flight actions for bulk transfer | Phase 7A Arrow Flight |

---

## 5. Dependency Graph

```
neo4j-rs
  └── ladybug-contract (optional, feature = "ladybug")
        ├── Container          (8192-bit vector, all ops)
        ├── CogRecord          (meta + content)
        ├── ContainerGeometry  (Cam/Xyz/Bridge/Extended/Chunked/Tree)
        ├── MetaView/Mut       (zero-copy metadata access)
        ├── TruthValue         (NARS truth functions)
        ├── CogPacket          (wire protocol)
        └── CognitiveAddress   (64-bit address)

Note: neo4j-rs does NOT depend on ladybug (main crate).
      Only the contract crate. This keeps the dependency minimal:
      pure types, no I/O, no storage, no network.
```

---

## 6. What NOT to Scaffold

Resist the temptation to scaffold:

1. **SpineCache in neo4j-rs** — SpineCache lives in ladybug main crate.
   LadybugBackend creates a local SpineCache instance but the type
   comes from a future in-process integration (Phase 7A), not now.

2. **HdrCascadeExec in neo4j-rs** — The HDR cascade is a ladybug search
   primitive. In Phase 4, vector_query() calls into ladybug-rs via HTTP
   or Arrow Flight. In Phase 7A, it calls in-process.

3. **CogRedis protocol** — neo4j-rs talks Cypher, not Redis. The CogRedis
   layer is internal to ladybug-rs.

4. **BindSpace addressing** — neo4j-rs uses NodeId(u64). The mapping to
   Addr(prefix:slot) happens inside LadybugBackend, invisible to the rest
   of neo4j-rs.

5. **New ContainerGeometry variants** — The existing 6 cover all cases.
   The Jina hybrid uses `Bridge` geometry (CAM proxy + external vector).

---

## 7. Success Criteria

The integration is complete when:

1. `cargo build --features ladybug` compiles neo4j-rs with ladybug-contract
2. `LadybugBackend` passes all existing MemoryBackend test cases
3. `aiwar_full.cypher` loads through LadybugBackend with correct results
4. `CALL ladybug.search(fp, k)` returns resonance-ranked results
5. NARS truth values accumulate from aiwar relationship evidence
6. No changes to ladybug-contract were needed (types were already correct)

---

## 8. Timeline

```
Week 1: Phase A (dep wiring) + Phase B (translation layer)
Week 2: Phase C (LadybugBackend scaffold) + Phase D (acceptance test)
Week 3: Phase E (NARS pipeline) + polish + integration testing
```

Total: ~12 working days for a fully functional LadybugBackend with
NARS calibration from aiwar data.

---

*This plan builds entirely on frozen ladybug-contract types. Zero schema
changes needed. The architecture is already in the code — this plan just
wires neo4j-rs to it.*
