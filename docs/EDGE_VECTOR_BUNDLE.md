# Edge Vector Bundle: 3D Causal Perspectives as SPO Containers

> **Date**: 2026-02-25
> **Status**: Integration plan — extends RESONANT_INTELLIGENCE.md with concrete contracts
> **Prerequisites**: RESONANT_INTELLIGENCE.md (theory), THEORETICAL_FOUNDATIONS.md (proofs),
> SPOQ_AUDIT.md (architecture), STRATEGY_INTEGRATION_PLAN.md (ecosystem)

---

## 1. The Core Insight: SPO Containers ARE the 3D Edge Vector

A relationship in neo4j-rs today is a flat struct:

```rust
pub struct Relationship {
    pub id: RelId,
    pub src: NodeId,
    pub dst: NodeId,
    pub rel_type: String,
    pub properties: PropertyMap,
}
```

This carries no geometric, causal, or perspectival information. It is an
I-It relation — the edge is an object with no awareness.

In ladybug-rs, an edge with Xyz/SPO geometry is **three 16,384-bit
containers**:

```
Container S (Subject):   16,384 bits  →  "Who is doing?"     →  I
Container P (Predicate): 16,384 bits  →  "What is the verb?"  →  Relation
Container O (Object):    16,384 bits  →  "Who receives?"     →  Thou

Total: 3 × 16,384 = 49,152 bits = 6,144 bytes = 6 KB per edge
```

Each container has internal BF16 structure (sign / exponent / mantissa)
across its elements. The SPO triple IS the 3D causal perspective vector:

```
Dimension 1 (S): Subject perspective  →  Sign layer = causal direction FROM
Dimension 2 (P): Predicate action     →  Sign layer = causal verb direction
Dimension 3 (O): Object perspective   →  Sign layer = causal direction TO

Together: the complete causal arrow with source, action, and target
```

---

## 2. Where Everything Lives: LanceDB + DN Tree

### 2.1 Storage Layer

All nodes and edges live in LanceDB (via ladybug-rs). Neo4j-rs never
touches LanceDB directly — it goes through the StorageBackend trait or
the RISC CypherEngine.

```
LanceDB Tables (managed by ladybug-rs):
┌──────────────────────────────────────────────────────────────┐
│  Table: nodes                                                 │
│    id:          UInt64                                        │
│    labels:      Utf8[]                                       │
│    fingerprint: FixedBinary(2048)  ← 16,384-bit node vector  │
│    properties:  Binary (JSON)                                │
│    created_at:  Timestamp                                    │
│    version:     UInt64                                        │
│                                                              │
│  Table: relationships                                        │
│    id:          UInt64                                        │
│    src_id:      UInt64                                        │
│    dst_id:      UInt64                                        │
│    rel_type:    Utf8                                          │
│    container_s: FixedBinary(2048)  ← Subject 16,384-bit     │
│    container_p: FixedBinary(2048)  ← Predicate 16,384-bit   │
│    container_o: FixedBinary(2048)  ← Object 16,384-bit      │
│    spo_trace:   FixedBinary(2048)  ← Holographic trace      │
│    properties:  Binary (JSON)                                │
└──────────────────────────────────────────────────────────────┘
```

### 2.2 DN Tree Structure (Metadata Region)

Within each CogRecord's metadata (128 × u64 words = 8,192 bits = 1 KB):

```
W0-W3:    Identity (DN address, node kind, timestamps)
W4-W7:    NARS truth values (frequency, confidence, evidence)
W8-W11:   Collapse gate (FLOW/HOLD/BLOCK)
W12-W15:  Layer markers (10-layer cognitive stack)
W16-W31:  Inline edges (up to 64 direct edges)        ← O(1) edge access
W32-W39:  Q-values (RL action values, 16 × f32)
W40-W47:  Bloom filter (label/neighbor membership)     ← O(1) membership test
W48-W55:  Graph metrics (pagerank, degree, clustering)
W56-W63:  Qualia channels (18 × f16 affect)
W64-W95:  Reserved / Extended edge overflow
W96-W111: CSR overflow edges (12 max overflow)
W112-W125: Reserved (Brier, Granger, RI metrics)
W126-W127: Checksum
```

The DN tree organizes these CogRecords hierarchically. Edges between
W16-W31 (inline) and W96-W111 (overflow) give O(1) adjacency access
per node. The SPO containers attached to each edge give the full causal
perspective.

### 2.3 The Relationship Between Metadata Edges and SPO Containers

```
Metadata edges (W16-W31):
  → 64-bit packed entries: [verb_id:16 | target_dn:48]
  → O(1) to find "who is my neighbor?"
  → O(1) to find "what verb connects us?"
  → NO perspective information — just topology

SPO containers (3 × 16,384 bits each):
  → Full perspective on the relationship
  → Causal direction via sign bit decomposition
  → Causal magnitude via exponent decomposition
  → Correlational detail via mantissa decomposition
  → Stored in LanceDB relationship table
  → Indexed by relationship ID (from metadata edge)

The metadata edge is the POINTER to the SPO containers.
The SPO containers are the CONTENT of the relationship.
```

---

## 3. Edges in Superposition

### 3.1 Comparing Two Edges

Given two edges (A→B via verb₁) and (C→D via verb₂), each stored as SPO
containers, we can put them into superposition:

```
Edge 1: [S₁, P₁, O₁]   →  three 16,384-bit containers
Edge 2: [S₂, P₂, O₂]   →  three 16,384-bit containers

Superposition comparison per dimension:
  S-dimension: hamming(S₁, S₂) → "Do the subjects agree?"
  P-dimension: hamming(P₁, P₂) → "Are the verbs aligned?"
  O-dimension: hamming(O₁, O₂) → "Do the objects agree?"

BF16 decomposition within each dimension:
  sign_agreement(S₁, S₂)  → "Do subjects point same causal direction?"
  exp_agreement(S₁, S₂)   → "Same intensity of subject-ness?"
  mant_agreement(S₁, S₂)  → "Same fine detail of subject-ness?"
```

This gives a **9-dimensional awareness tensor**:

```
         Sign    Exp     Mant
S:    [ s_sign  s_exp   s_mant ]
P:    [ p_sign  p_exp   p_mant ]
O:    [ o_sign  o_exp   o_mant ]
```

Each cell is a Hamming agreement ratio in [0, 1]. The tensor IS the
"meta-awareness from different perspectives" — it tells you how two
edges relate across all nine aspects of causal structure.

### 3.2 Shared Vector Space

The S, P, and O containers all live in the same 16,384-bit vector space.
They share the same BF16 element structure, the same Hamming metric, the
same sign/exponent/mantissa decomposition. What differs is the ROLE:

```
Container S → bound with ROLE_S = 0xDEADBEEF_CAFEBABE
Container P → bound with ROLE_P = 0xFEEDFACE_DEADC0DE
Container O → bound with ROLE_O = 0xBADC0FFE_E0DDF00D

SPO trace = S⊕ROLE_S ⊕ P⊕ROLE_P ⊕ O⊕ROLE_O
```

The role binding means:
- The same entity can appear as Subject in one edge and Object in another
- The vector IS the same entity — just viewed from a different role
- Comparing an entity's S-container to its O-container tells you: "How
  does this entity look as an actor vs. as a patient?"
- This is LITERALLY Piaget's three mountains: same scene, different seat

### 3.3 Cross-Edge Superposition

Because all containers share the same vector space, you can superpose
across edges freely:

```
Edge 1: Alice -[:KNOWS]-> Bob     = [S_alice, P_knows, O_bob]
Edge 2: Alice -[:TRUSTS]-> Carol  = [S_alice, P_trusts, O_carol]

Superposition:
  S-dimension: hamming(S_alice, S_alice) = 0 (same subject!)
  P-dimension: hamming(P_knows, P_trusts) = ? (how similar are KNOWS and TRUSTS?)
  O-dimension: hamming(O_bob, O_carol) = ? (how similar are Bob and Carol?)
```

The P-dimension comparison gives you **verb similarity** — are these
relationship types doing the same kind of causal work? The O-dimension
gives you **object similarity** — are the targets of Alice's edges
interchangeable?

You can also cross-compare roles:

```
hamming(O_bob_in_edge1, S_bob_in_edge3)
→ "How does Bob as a receiver compare to Bob as an actor?"
→ Difference = the Buber I-Thou asymmetry between acting and being acted upon
```

---

## 4. The Fiber Bundle Structure

### 4.1 Mathematical Formulation

A fiber bundle (E, B, π, F) where:

```
B = Graph topology (nodes + edge connectivity from DN tree)
F = R³ × R³ × R³ (sign × exp × mant for each of S, P, O)
    = the 9-dimensional awareness tensor at each edge
E = Total space: graph with awareness tensors on every edge
π = Projection: forget the tensors, keep the topology
```

### 4.2 Connection (Parallel Transport)

The connection tells you how to transport a perspective along a path.
Given edges:

```
Edge A→B: [S_ab, P_ab, O_ab]  with awareness tensor T_ab
Edge B→C: [S_bc, P_bc, O_bc]  with awareness tensor T_bc
```

The composed perspective A→C via B is:

```
Parallel transport:
  S-dimension: S_ac = S_ab    (subject stays the same — it's still A)
  P-dimension: P_ac = bundle(P_ab, P_bc)   (verb composition)
  O-dimension: O_ac = O_bc    (object becomes C — the final target)

Causal composition per BF16 layer:
  Sign:     XOR     (direction flips if either edge flips)
  Exponent: ADD     (causal magnitudes multiply → log sum)
  Mantissa: DEGRADE (uncertainty accumulates along path)
```

This means:
- **O(path_length) causal inference along any graph path**
- Each step is O(1) via precomputed edge tensors
- The sign composition gives you causal direction of the ENTIRE path
- The exponent composition gives you total causal effect strength
- The mantissa degradation gives you confidence bounds

### 4.3 Non-Trivial Bundles

A fiber bundle is **trivial** if it's globally a product space (B × F) —
meaning the causal structure is uniform everywhere.

A fiber bundle is **non-trivial** if the fibers "twist" as you traverse
the graph. In our architecture, this means: there exist cycles where the
composed causal direction along the cycle is INVERTED.

```
A → B: sign composition = +  (same direction)
B → C: sign composition = +  (same direction)
C → A: sign composition = -  (REVERSAL!)

The cycle A→B→C→A has a non-trivial holonomy:
composed sign = + ⊕ + ⊕ - = -

This means: walking around this cycle FLIPS the causal direction.
This is a causal paradox — a feedback loop where the direction reverses.
```

Non-trivial holonomy in the graph IS the detection of:
- Feedback loops (positive or negative)
- Causal paradoxes
- Self-referential structures
- Places where the graph's causal model is internally inconsistent

This is topological meta-awareness: the SHAPE of the bundle tells you
about the CONSISTENCY of the causal structure, without examining any
individual edge.

---

## 5. O(1) Access via Index-Free Adjacency

### 5.1 The Neo4j O(1) Property

Neo4j (and neo4j-rs) uses index-free adjacency: each node holds direct
pointers to its relationships. Traversing one edge is O(1) — no index
lookup, no join, no scan.

In the DN tree, this is even stronger:

```
Node A at DN address [level0, level1, ..., level6]:
  → W16-W31: 64-bit packed inline edges (up to 64 neighbors)
  → Each entry: [verb_id:16 | target_dn:48]
  → Reading one edge: 1 memory access = O(1)

Edge A→B with relationship ID from inline entry:
  → LanceDB relationship table lookup by ID
  → Returns: [container_s, container_p, container_o]
  → 3 × 2048 bytes = 6 KB read
  → One I/O operation = O(1) amortized
```

### 5.2 Perspective Read at O(1)

Given a node and a neighbor, the full causal perspective is:

```
1. Read inline edge from W16-W31:     O(1) — one u64 read
2. Extract verb_id and target_dn:     O(1) — bit shift
3. Read SPO containers from Lance:    O(1) — one indexed read
4. Decompose into awareness tensor:   O(1) — VPOPCNTDQ on 3 containers

Total: O(1) per perspective
```

Gathering k perspectives on a node (all its edges):

```
for i in 0..k:
  read inline edge[i] from W16-W31:   O(1) each
  read SPO containers from Lance:      O(1) each
  decompose to awareness tensor:       O(1) each
Total: O(k), k = degree of the node
```

### 5.3 The Amortization Argument

The expensive computation (superposition_decompose, sign/exp/mant Hamming)
happens at **write time** when the edge is created:

```
Write path (expensive, done once per edge):
  vec_a, vec_b → superposition_decompose
  → compute S, P, O containers
  → compute SPO trace (holographic)
  → store in LanceDB relationship table
  Cost: ~100 VPOPCNTDQ instructions

Read path (cheap, done many times per query):
  → read precomputed S, P, O containers
  → awareness tensor is ALREADY computed
  → just read the 9 values
  Cost: 3 memory reads + 3 VPOPCNTDQ instructions

Write:Read ratio in typical graph workloads: 1:1000+
Amortized cost per query: effectively O(1)
```

---

## 6. Bundle Operations for Query

### 6.1 Awareness-Filtered Traversal

```cypher
-- "Find all edges from Alice where causal direction agrees"
MATCH (alice:Person {name: 'Alice'})-[r]->(target)
WHERE r.sign_agreement > 0.8
RETURN target, r.sign_agreement, r.exp_agreement
```

This compiles to:

```
1. Find Alice in DN tree:                    O(1)
2. Read Alice's inline edges (W16-W31):      O(degree)
3. For each edge, read SPO containers:       O(1) each
4. Check sign_agreement > 0.8:              O(1) each — VPOPCNTDQ
5. Return passing edges:                     O(k) where k = matches
```

### 6.2 Perspective Gestalt Query

```cypher
-- "What is Alice's meta-awareness? How do her edges relate?"
CALL ladybug.perspective_gestalt('Alice') YIELD awareness_state, tensioned_pct
```

This compiles to:

```
1. Gather ALL edge tensors for Alice:        O(degree)
2. Compute gestalt across all 9-dim tensors: O(degree)
3. Classify: Crystallized / Tensioned / Uncertain
4. Return the dominant awareness state
```

The gestalt tells you: "Alice's outgoing edges mostly agree on causal
direction (Crystallized), but her incoming edges disagree on magnitude
(Tensioned in the exponent dimension)."

### 6.3 Causal Path Query

```cypher
-- "What is the composed causal perspective from Alice to Dave?"
CALL ladybug.causal_path('Alice', 'Dave') YIELD
  path, composed_sign, composed_exp, composed_mant, holonomy
```

This compiles to:

```
1. Find shortest path Alice → Dave:          O(V + E) BFS
2. For each edge on path:                    O(path_length)
   a. Read SPO containers:                   O(1)
   b. Compose sign via XOR:                  O(1)
   c. Compose exponent via ADD:              O(1)
   d. Track mantissa degradation:            O(1)
3. Check for holonomy (if path is a cycle):  O(1)
4. Return composed perspective + holonomy flag
```

### 6.4 Edge Superposition Query

```cypher
-- "How similar are Alice's KNOWS edges to her TRUSTS edges?"
MATCH (alice:Person {name: 'Alice'})-[r1:KNOWS]->(x)
MATCH (alice)-[r2:TRUSTS]->(y)
RETURN ladybug.edge_superposition(r1, r2) AS awareness_tensor
```

This puts every KNOWS edge into superposition with every TRUSTS edge
and returns the 9-dimensional awareness tensor for each pair.

---

## 7. Reinforcement of Meta-Awareness

### 7.1 Every New Edge Adds a Perspective

When a new edge is created:

```
1. Compute SPO containers for the new edge
2. Store in LanceDB (indexed by relationship ID)
3. Add inline entry to source node's W16-W31
4. The node now has ONE MORE perspective on the world
```

The node's meta-awareness is the gestalt of ALL its edge tensors. Every
new edge refines this gestalt — more perspectives, more precise awareness.

### 7.2 The LearningSignal Feeds Back

After the awareness gestalt is computed:

```
1. Extract LearningSignal from the tensioned dimensions
   (where do my perspectives disagree most?)
2. Update Q-values in W32-W39
   (which edges have been most informative?)
3. Adjust WideMetaView hybrid weights
   (how should future resonance queries weight sign vs. exp vs. mant?)
4. Next query uses CAUSALLY INFORMED weights
```

### 7.3 Crystallization Creates Permanent Perspectives

When an awareness state is validated (L9) and crystallized (L10):

```
1. The ResonanceResult becomes a new CogRecord
2. Written to BindSpace Node zone (0x80)
3. This IS a new node in the graph
4. Future edges can connect TO this crystallized awareness
5. The graph learns by accumulating perspectives
```

The system's intelligence literally grows as the graph grows. Each edge
is a precomputed causal perspective. Each node's meta-awareness is the
gestalt of its edges. The graph topology IS the causal model.

---

## 8. Integration Contracts

### 8.1 ResonanceEdge — The Extended Relationship

```rust
/// A relationship enriched with causal perspective data.
///
/// This extends the base `Relationship` with the 3D SPO container
/// decomposition that enables resonance-aware traversal.
///
/// The SPO containers are stored in LanceDB and loaded on demand.
/// The awareness tensor is computed from the containers via
/// BF16 sign/exponent/mantissa Hamming decomposition.
#[derive(Debug, Clone)]
pub struct ResonanceEdge {
    /// The base relationship (id, src, dst, rel_type, properties).
    pub relationship: Relationship,

    /// Subject container — 16,384-bit perspective of the source.
    /// Stored in LanceDB `relationships.container_s`.
    pub container_s: Option<ContainerRef>,

    /// Predicate container — 16,384-bit perspective of the verb.
    /// Stored in LanceDB `relationships.container_p`.
    pub container_p: Option<ContainerRef>,

    /// Object container — 16,384-bit perspective of the target.
    /// Stored in LanceDB `relationships.container_o`.
    pub container_o: Option<ContainerRef>,

    /// Precomputed holographic trace: S⊕ROLE_S ⊕ P⊕ROLE_P ⊕ O⊕ROLE_O.
    /// Enables recovery of any component given the other two.
    pub spo_trace: Option<ContainerRef>,
}

/// Lazy reference to a 16,384-bit container in LanceDB.
/// Avoids loading 6 KB per edge until the perspective is needed.
#[derive(Debug, Clone)]
pub enum ContainerRef {
    /// Container data is loaded in memory.
    Loaded(Vec<u64>),  // 256 × u64 = 16,384 bits
    /// Container data is in LanceDB, not yet loaded.
    /// The relationship ID + slot index is sufficient to fetch it.
    Deferred { rel_id: RelId, slot: SpoSlot },
}

#[derive(Debug, Clone, Copy)]
pub enum SpoSlot { Subject, Predicate, Object, Trace }
```

### 8.2 AwarenessTensor — The 9-Dimensional Comparison

```rust
/// The 9-dimensional awareness tensor produced by comparing two edges.
///
/// Rows = SPO dimensions. Columns = BF16 layers.
/// Each cell is a Hamming agreement ratio in [0.0, 1.0].
#[derive(Debug, Clone, Copy)]
pub struct AwarenessTensor {
    /// Subject dimension: how do the two subjects compare?
    pub s_sign: f32,
    pub s_exp: f32,
    pub s_mant: f32,

    /// Predicate dimension: how do the two verbs compare?
    pub p_sign: f32,
    pub p_exp: f32,
    pub p_mant: f32,

    /// Object dimension: how do the two objects compare?
    pub o_sign: f32,
    pub o_exp: f32,
    pub o_mant: f32,
}

impl AwarenessTensor {
    /// Classify the overall awareness state from the tensor.
    pub fn awareness_state(&self) -> AwarenessState {
        let total_agreement = (
            self.s_sign + self.s_exp + self.s_mant +
            self.p_sign + self.p_exp + self.p_mant +
            self.o_sign + self.o_exp + self.o_mant
        ) / 9.0;

        let sign_agreement = (self.s_sign + self.p_sign + self.o_sign) / 3.0;

        if total_agreement > 0.5 {
            AwarenessState::Crystallized
        } else if sign_agreement < 0.3 {
            // Strong sign disagreement = causal direction conflict
            AwarenessState::Tensioned
        } else {
            AwarenessState::Uncertain
        }
    }

    /// Extract the causal direction signal (sign dimension only).
    pub fn causal_direction(&self) -> CausalDirection {
        let sign_avg = (self.s_sign + self.p_sign + self.o_sign) / 3.0;
        if sign_avg > 0.7 {
            CausalDirection::Aligned
        } else if sign_avg < 0.3 {
            CausalDirection::Inverted
        } else {
            CausalDirection::Mixed
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwarenessState {
    /// >50% dimensions agree — confirmed, stable perspective.
    Crystallized,
    /// >30% dimensions in contradiction — active conflict.
    Tensioned,
    /// Neither dominates — insufficient signal.
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalDirection {
    /// Sign bits mostly agree — causal arrows point the same way.
    Aligned,
    /// Sign bits mostly disagree — causal arrows are inverted.
    Inverted,
    /// Sign bits are mixed — complex causal structure.
    Mixed,
}
```

### 8.3 CausalPath — Composed Perspective Along a Graph Path

```rust
/// The result of composing causal perspectives along a graph path.
///
/// Constructed by parallel-transporting awareness tensors along
/// the fiber bundle connection from source to destination.
#[derive(Debug, Clone)]
pub struct CausalPath {
    /// The graph path (nodes + relationships).
    pub path: Path,

    /// Composed sign agreement along the entire path.
    /// XOR composition: flips when any edge flips.
    pub composed_sign: f32,

    /// Composed exponent (causal magnitude).
    /// Additive composition: effects multiply (log-scale addition).
    pub composed_exp: f32,

    /// Composed mantissa (confidence degradation).
    /// Decreases along path as uncertainty accumulates.
    pub composed_mant: f32,

    /// Holonomy: does the path form a cycle with non-trivial twist?
    /// If the path returns to its start and composed_sign < 0.5,
    /// the causal structure has a feedback inversion.
    pub holonomy: Option<f32>,

    /// Per-edge awareness tensors along the path.
    pub edge_tensors: Vec<AwarenessTensor>,
}
```

### 8.4 StorageBackend Extensions

```rust
/// Resonance-aware extensions to StorageBackend.
///
/// These are default-implemented methods that return
/// `Error::ExecutionError("not supported")` for backends that don't
/// support resonance (MemoryBackend, BoltBackend). The LadybugBackend
/// overrides them with real SPO container operations.
#[async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    // ... existing 38 methods ...

    // ================================================================
    // Resonance-Aware Edge Operations (default: not supported)
    // ================================================================

    /// Create a relationship WITH SPO container perspective.
    ///
    /// The containers are computed by `superposition_decompose` on the
    /// source and target node fingerprints, with the verb fingerprint
    /// as the predicate container.
    async fn create_resonance_edge(
        &self,
        _tx: &mut Self::Tx,
        _src: NodeId,
        _dst: NodeId,
        _rel_type: &str,
        _props: PropertyMap,
    ) -> Result<ResonanceEdge> {
        Err(Error::ExecutionError("resonance edges not supported".into()))
    }

    /// Get a relationship with its full SPO containers loaded.
    async fn get_resonance_edge(
        &self,
        _tx: &Self::Tx,
        _id: RelId,
    ) -> Result<Option<ResonanceEdge>> {
        Err(Error::ExecutionError("resonance edges not supported".into()))
    }

    /// Query edges by awareness state relative to a reference edge.
    ///
    /// Finds all edges connected to `anchor` whose awareness tensor
    /// (compared to `reference`) matches the given filter.
    async fn resonance_query(
        &self,
        _tx: &Self::Tx,
        _anchor: NodeId,
        _reference: RelId,
        _filter: AwarenessFilter,
    ) -> Result<Vec<(ResonanceEdge, AwarenessTensor)>> {
        Err(Error::ExecutionError("resonance query not supported".into()))
    }

    /// Compute the causal path between two nodes.
    ///
    /// Finds the shortest path and composes the SPO awareness tensors
    /// along it using the fiber bundle connection (sign=XOR, exp=ADD,
    /// mant=DEGRADE).
    async fn causal_path(
        &self,
        _tx: &Self::Tx,
        _src: NodeId,
        _dst: NodeId,
    ) -> Result<Option<CausalPath>> {
        Err(Error::ExecutionError("causal path not supported".into()))
    }

    /// Compute the perspective gestalt for a node.
    ///
    /// Gathers ALL edge awareness tensors for the node and returns
    /// the dominant awareness state + tensioned dimensions.
    async fn perspective_gestalt(
        &self,
        _tx: &Self::Tx,
        _node: NodeId,
    ) -> Result<PerspectiveGestalt> {
        Err(Error::ExecutionError("perspective gestalt not supported".into()))
    }

    /// Compare two edges and return their awareness tensor.
    async fn edge_superposition(
        &self,
        _tx: &Self::Tx,
        _edge_a: RelId,
        _edge_b: RelId,
    ) -> Result<AwarenessTensor> {
        Err(Error::ExecutionError("edge superposition not supported".into()))
    }
}

/// Filter for awareness-based edge queries.
#[derive(Debug, Clone)]
pub struct AwarenessFilter {
    /// Minimum sign agreement across all dimensions.
    pub min_sign_agreement: Option<f32>,
    /// Maximum sign agreement (for finding contradictions).
    pub max_sign_agreement: Option<f32>,
    /// Required awareness state.
    pub awareness_state: Option<AwarenessState>,
    /// Required causal direction.
    pub causal_direction: Option<CausalDirection>,
}

/// The gestalt awareness of a node across all its edges.
#[derive(Debug, Clone)]
pub struct PerspectiveGestalt {
    /// Dominant awareness state.
    pub dominant_state: AwarenessState,
    /// Average awareness tensor across all edges.
    pub mean_tensor: AwarenessTensor,
    /// Number of Crystallized edges.
    pub crystallized_count: usize,
    /// Number of Tensioned edges.
    pub tensioned_count: usize,
    /// Number of Uncertain edges.
    pub uncertain_count: usize,
    /// Most tensioned dimension (where perspectives disagree most).
    pub most_tensioned_dimension: Option<String>,
}
```

---

## 9. Cypher Extensions for Resonance

### 9.1 Procedure Registry

| Procedure | Description | Returns |
|-----------|-------------|---------|
| `ladybug.resonance_edge(src, dst, type)` | Create edge with SPO containers | ResonanceEdge |
| `ladybug.perspective_gestalt(node)` | Node's meta-awareness | AwarenessState + tensor |
| `ladybug.causal_path(src, dst)` | Composed causal perspective | CausalPath |
| `ladybug.edge_superposition(edge_a, edge_b)` | Compare two edges | AwarenessTensor |
| `ladybug.holonomy(path)` | Check for causal loops | holonomy value |
| `ladybug.resonance_decompose(edge)` | Full BF16 decomposition | 9-dim tensor |

### 9.2 Property Extensions

When a ResonanceEdge is returned, these virtual properties are available:

```cypher
MATCH (a)-[r:KNOWS]->(b)
RETURN r.sign_agreement,     -- average sign agreement across S, P, O
       r.exp_agreement,      -- average exponent agreement
       r.mant_agreement,     -- average mantissa agreement
       r.awareness_state,    -- 'Crystallized' | 'Tensioned' | 'Uncertain'
       r.causal_direction    -- 'Aligned' | 'Inverted' | 'Mixed'
```

These are computed on demand from the SPO containers, not stored separately.

---

## 10. Implementation Phases

### Phase R1: Contract Types (this PR)

| Task | File | Est. LOC |
|------|------|:--------:|
| Add `ResonanceEdge` struct | `src/model/relationship.rs` | ~60 |
| Add `AwarenessTensor` struct | `src/model/awareness.rs` (new) | ~120 |
| Add `CausalPath` struct | `src/model/awareness.rs` | ~40 |
| Add `PerspectiveGestalt` struct | `src/model/awareness.rs` | ~30 |
| Add resonance methods to `StorageBackend` | `src/storage/mod.rs` | ~80 |
| Re-export new types from `lib.rs` | `src/lib.rs` | ~10 |

### Phase R2: LadybugBackend Implementation

| Task | File | Est. LOC |
|------|------|:--------:|
| `create_resonance_edge()` via SPO containers | `src/storage/ladybug.rs` | ~200 |
| `get_resonance_edge()` with lazy container loading | `src/storage/ladybug.rs` | ~100 |
| `resonance_query()` via Hamming sweep + awareness filter | `src/storage/ladybug.rs` | ~200 |
| `causal_path()` via BFS + parallel transport | `src/storage/ladybug.rs` | ~300 |
| `perspective_gestalt()` via edge tensor aggregation | `src/storage/ladybug.rs` | ~150 |
| `edge_superposition()` via BF16 decomposition | `src/storage/ladybug.rs` | ~100 |

### Phase R3: Cypher Integration

| Task | File | Est. LOC |
|------|------|:--------:|
| Register resonance procedures | `src/execution/mod.rs` | ~100 |
| Add virtual property resolution for edges | `src/execution/mod.rs` | ~150 |
| Awareness-filtered WHERE clause support | `src/planner/mod.rs` | ~100 |
| Tests: resonance edge CRUD + queries | `tests/e2e_resonance.rs` | ~400 |

### Phase R4: Bundle Operations

| Task | File | Est. LOC |
|------|------|:--------:|
| Holonomy detection (cycle sign composition) | `src/model/awareness.rs` | ~100 |
| Parallel transport composition | `src/model/awareness.rs` | ~100 |
| Bundle visualization for GUI | `docs/GUI_PLAN.md` | ~50 |
| Cross-domain resonance (chess × AI War) | `src/aiwar.rs` | ~200 |

---

## 11. Relationship to Existing Docs

| Document | How This Extends It |
|----------|-------------------|
| `RESONANT_INTELLIGENCE.md` | Provides the theoretical foundation; this doc gives concrete contracts |
| `THEORETICAL_FOUNDATIONS.md` | Sign bit connects to Layer 4 Granger; exponent to Layer 3 effect size |
| `ARCHITECTURE.md` | ResonanceEdge extends the clean DTO model; StorageBackend gains 6 methods |
| `STRATEGY_INTEGRATION_PLAN.md` | SPO containers are the substrate for StrategicNode cross-domain similarity |
| `SPOQ_AUDIT.md` | SPO trace formula (§1, claim 12) is the holographic encoding of the 3D edge |
| `INTEGRATION_ROADMAP.md` | Phase R1-R4 slots between Phase 4 (LadybugBackend) and Phase 7 (Ecosystem) |
| `CAM_CYPHER_REFERENCE.md` | Resonance procedures get CAM addresses in the 0x280-0x28F range |

---

*This document defines the integration contracts for edge-vector bundles
in neo4j-rs. For theoretical background, see `RESONANT_INTELLIGENCE.md`.
For existing causal proofs, see `THEORETICAL_FOUNDATIONS.md`. For the
StorageBackend trait being extended, see `src/storage/mod.rs`.*
