# Holograph AVX-512 Integration Plan: 4×16384-bit CogRecord

> **Date**: 2026-02-21
> **Author**: AdaWorldAPI
> **Status**: Architecture finalized. Implementation plan for Claude Code.
> **Supersedes**: HOLOGRAPH_CONTRACT.md §1 (Container at 4×8192 bits)
> **Required reading before this doc**: None. This IS the starting doc.

---

## 0. What This Document Is

This is a self-contained integration plan for upgrading the Holograph property
graph database from 4×8192-bit containers (4 KB CogRecord) to **4×16384-bit
containers (8 KB CogRecord)** using AVX-512 VPOPCNTDQ hardware acceleration.

The change doubles the information capacity per node while maintaining
sub-microsecond query times, because the target hardware has native 512-bit
popcount instructions that process 8 u64 lanes per cycle.

### Repositories Involved

| Repo | Role | URL |
|------|------|-----|
| **ada-docs** | Architecture docs (you are reading one) | `AdaWorldAPI/ada-docs` |
| **neo4j-rs** | Cypher compiler → direct BindSpace ops | `AdaWorldAPI/neo4j-rs` |
| **ladybug-rs** | Substrate: BindSpace, Container, SIMD, NARS | `AdaWorldAPI/ladybug-rs` |
| **holograph** | One-binary workspace containing both crates | `AdaWorldAPI/holograph` |

### PAT for GitHub API Access

```
$GITHUB_PAT (see ada-docs secure config)
```

Always use zipball + REST API for repo access. Never `git clone`.

---

## 1. Hardware Foundation

The target deployment environment has the following AVX-512 extensions:

```
avx512f            Base 512-bit integer/float
avx512bw           Byte/word operations
avx512cd           Conflict detection
avx512dq           Doubleword/quadword
avx512vl           128/256-bit vector length variants
avx512_vpopcntdq   ★ Native 512-bit popcount on u64 lanes
avx512_vnni        ★ Int8 multiply-accumulate (VPDPBUSD)
avx512_bitalg      ★ Bit manipulation (VPSHUFBITQMB)
avx512_vbmi        Variable byte shuffle
```

### What This Means

| Instruction | What it does | Impact |
|-------------|-------------|--------|
| `VPOPCNTQ zmm` | Popcount 8×u64 in one cycle | Hamming distance on 16384 bits = 32 instructions |
| `VPXORQ zmm` | XOR 512 bits in one cycle | BIND on 16384 bits = 32 instructions |
| `VPDPBUSD zmm` | Int8 dot product, 64 lanes | Cosine similarity on int8 embeddings, native |
| `VPSHUFBITQMB` | Bit permutation via shuffle | Codebook lookup acceleration |

**Key insight**: With VPOPCNTDQ, doubling the container from 8192→16384 bits
costs exactly 2× in popcount cycles (~32 vs ~16 instructions), but the
hardware executes these at 1 cycle each with full pipelining. A full 16384-bit
Hamming distance completes in ~35ns. A 65536-bit CogRecord distance = ~140ns.

---

## 2. The New CogRecord: 4×16384 bits = 8 KB

### Current Layout (HOLOGRAPH_CONTRACT.md)

```rust
struct CogRecord {
    meta:    [u64; 128],  // 8,192 bits = 1 KB
    block_0: [u64; 128],  // 8,192 bits = 1 KB
    block_1: [u64; 128],  // 8,192 bits = 1 KB
    block_2: [u64; 128],  // 8,192 bits = 1 KB
}
// Total: 4 × 8,192 = 32,768 bits = 4 KB
```

### New Layout

```rust
const CONTAINER_BITS: usize = 16_384;
const CONTAINER_LANES: usize = 256;  // 16384 / 64
const CONTAINER_BYTES: usize = 2048; // 16384 / 8

#[repr(C, align(64))]
struct Container {
    lanes: [u64; CONTAINER_LANES],  // 256 × u64 = 2 KB
}

#[repr(C, align(64))]
struct CogRecord {
    meta:    Container,  // 16,384 bits = 2 KB — structured
    block_0: Container,  // 16,384 bits = 2 KB — CAM content
    block_1: Container,  // 16,384 bits = 2 KB — B-tree / index
    block_2: Container,  // 16,384 bits = 2 KB — embedding store
}
// Total: 4 × 16,384 = 65,536 bits = 8 KB
```

### What Each Container Holds

```
┌─────────────────────────────────────────────────────────────────┐
│ CogRecord: 8 KB (65,536 bits)                                   │
│                                                                  │
│ META (2 KB)                                                      │
│   Q1  W0-W63     CAM identity, NARS, codebook routing,          │
│                  timestamps, flags, ports, execution state       │
│   Q2  W64-W127   64 inline edge slots (doubled from 32)         │
│   Q3  W128-W191  64 child/member refs (doubled from 32)         │
│   Q4  W192-W255  Available for: concept overflow, attention      │
│                  mask, phase-shift cache, qualia snapshot        │
│                                                                  │
│ BLOCK 0 — CAM Content (2 KB)                                    │
│   Content-addressable fingerprint. 16,384-bit multi-hot          │
│   encoding over learned codebook vocabulary. Pure Hamming.       │
│   JL theorem: more bits = better distance preservation.          │
│   At 16K bits, expect ~1.4× intra/inter ratio (vs 1.22× at 8K) │
│                                                                  │
│ BLOCK 1 — B-Tree Index / Structural Position (2 KB)             │
│   DN path encoding + hierarchical position fingerprint.          │
│   Supports: parent/child/sibling O(1) via popcount.             │
│   Also: hashtag-everything zone for edge/verb/concept addresses  │
│                                                                  │
│ BLOCK 2 — Embedding Store (2 KB)                                │
│   Quantized dense embeddings. Options:                           │
│     int8  × 1024D =  8,192 bits (half container, room for meta) │
│     int8  × 2048D = 16,384 bits (full container)                │
│     int4  × 1024D =  4,096 bits (quarter, ultra-compact)        │
│     int4  × 4096D = 16,384 bits (full container, high-dim)      │
│     binary × 16384 = 16,384 bits (pure Hamming search)          │
│                                                                  │
│   Distance metrics (both hardware-accelerated):                  │
│     Hamming → VPOPCNTDQ (binary fingerprints)                   │
│     Dot/Cosine → VPDPBUSD/VNNI (int8 embeddings)               │
└─────────────────────────────────────────────────────────────────┘
```

### Container Geometries (unchanged semantics, doubled capacity)

| Geometry | Layout | Size | Use |
|----------|--------|------|-----|
| `Cam` | 1 meta + 1 content | 4 KB | Standard node |
| `Xyz` | 1 meta + 3 content | 8 KB | S + P + O searchable |
| `Extended` | 1 meta + 2 content | 6 KB | Primary + orthogonal |
| `Bridge` | 1 meta + 1 content | 4 KB | CAM proxy + external vector |

---

## 3. The Five RISC Operations — AVX-512 Implementations

### 3.1 BIND (XOR)

```rust
#[target_feature(enable = "avx512f")]
unsafe fn bind_avx512(a: &Container, b: &Container) -> Container {
    let mut out = Container::zero();
    for i in (0..CONTAINER_LANES).step_by(8) {
        let va = _mm512_loadu_epi64(a.lanes[i..].as_ptr() as *const i64);
        let vb = _mm512_loadu_epi64(b.lanes[i..].as_ptr() as *const i64);
        let vr = _mm512_xor_epi64(va, vb);
        _mm512_storeu_epi64(out.lanes[i..].as_mut_ptr() as *mut i64, vr);
    }
    out
}
// 256 lanes / 8 per instruction = 32 VPXORQ instructions
```

### 3.2 DISTANCE (Hamming via VPOPCNTDQ)

```rust
#[target_feature(enable = "avx512f,avx512_vpopcntdq")]
unsafe fn distance_avx512(a: &Container, b: &Container) -> u32 {
    let mut acc = _mm512_setzero_si512();
    for i in (0..CONTAINER_LANES).step_by(8) {
        let va = _mm512_loadu_epi64(a.lanes[i..].as_ptr() as *const i64);
        let vb = _mm512_loadu_epi64(b.lanes[i..].as_ptr() as *const i64);
        let xor = _mm512_xor_epi64(va, vb);
        let pop = _mm512_popcnt_epi64(xor);  // ★ THE INSTRUCTION
        acc = _mm512_add_epi64(acc, pop);
    }
    // Horizontal sum of 8 u64 lanes
    let mut total = [0u64; 8];
    _mm512_storeu_epi64(total.as_mut_ptr() as *mut i64, acc);
    total.iter().sum::<u64>() as u32
}
// 32 VPXORQ + 32 VPOPCNTQ + 32 VPADDQ + 1 horizontal sum
// ~100 instructions total, pipelined to ~35ns
```

### 3.3 BUNDLE (Ripple-Carry Bit-Parallel Majority Vote)

This is the key optimization. Proven at 22-40× speedup over naive in our
cam_vision benchmark (Feb 21, 2026).

```rust
fn bundle(words: &[&Container]) -> Container {
    let n = words.len();
    if n == 0 { return Container::zero(); }
    if n == 1 { return words[0].clone(); }

    let threshold = (n / 2) + 1;
    let nbits = (64 - (n as u64).leading_zeros()) as usize;

    // Stack-allocate counter: max 16 bits = supports up to 65535 input words
    // CRITICAL: Do NOT heap-allocate here. Vec causes 2000× overhead in hot path.
    let mut counter = [[0u64; CONTAINER_LANES]; 16];
    assert!(nbits <= 16);

    // Ripple-carry accumulation
    for w in words {
        for lane in 0..CONTAINER_LANES {
            let mut carry = w.lanes[lane];
            for k in 0..nbits {
                let new_carry = counter[k][lane] & carry;
                counter[k][lane] ^= carry;
                carry = new_carry;
                if carry == 0 { break; }
            }
        }
    }

    // Parallel threshold comparison via bitwise subtraction
    let mut out = Container::zero();
    let tv = threshold as u64;
    for lane in 0..CONTAINER_LANES {
        let mut borrow = 0u64;
        for k in 0..nbits {
            let tbit = if (tv >> k) & 1 == 1 { !0u64 } else { 0u64 };
            borrow = (!counter[k][lane] & tbit)
                   | (!counter[k][lane] & borrow)
                   | (tbit & borrow);
        }
        out.lanes[lane] = !borrow;
    }
    out
}
```

**Performance** (measured on cam_vision, 8192-bit, will scale linearly to 16384):

| N words | Optimized | Naive | Speedup |
|---------|-----------|-------|---------|
| 5 | 1.3 µs | 52.5 µs | 40× |
| 16 | 4.1 µs | 121 µs | 30× |
| 64 | 16.7 µs | 379 µs | 23× |
| 1024 | 288 µs | 8.0 ms | 28× |

**WARNING**: The ladybug-rs `simd_ops/mod.rs` ripple-carry currently runs at
~2.9ms constant overhead because it heap-allocates the counter via `Vec`. The
fix is stack allocation as shown above. See §7.1.

### 3.4 PERMUTE (Lane Rotation)

```rust
fn permute(w: &Container, k: usize) -> Container {
    if k == 0 { return w.clone(); }
    let lane_shift = k % CONTAINER_LANES;
    let mut out = Container::zero();
    for i in 0..CONTAINER_LANES {
        out.lanes[(i + lane_shift) % CONTAINER_LANES] = w.lanes[i];
    }
    out
}
```

Used for: role encoding in edges (permute(rel,1), permute(tgt,2)), creating
orthogonal crossing planes for the X-junction binding pattern.

### 3.5 SCAN (Radius Search)

Full scan of N records at 16384 bits each:

```
Time = N × distance_time ≈ N × 35ns
1M records: ~35ms (acceptable for full scan)
With signature pre-filter (16-bit codebook index): ~3.5ms
```

---

## 4. Embedding Store — Block 2 Details

### int8 × 1024D Layout

```
Block 2: 16,384 bits = 2,048 bytes

[0..1023]     int8 embedding values (1024 dimensions × 8 bits)
[1024..1055]  embedding metadata:
              [1024..1027] model_id (u32 — which CLIP/Jina model)
              [1028..1031] quantization_scale (f32)
              [1032..1035] quantization_zero_point (f32)
              [1036..1039] norm (f32 — original L2 norm before quant)
              [1040..1055] reserved
[1056..2047]  available (992 bytes for secondary embedding or features)
```

### Distance Computation

**Hamming distance on binary fingerprint (block_0)**:
```
VPOPCNTDQ: 32 instructions → ~35ns
```

**Dot product on int8 embedding (block_2)**:
```
VPDPBUSD (VNNI): 32 instructions → ~40ns
Cosine = dot / (norm_a × norm_b)  — norms stored in metadata
```

**Both in one query**:
```
Hamming filter on block_0 → top-K candidates (~3.5ms for 1M with pre-filter)
VNNI dot product on block_2 → rerank top-K (~40ns × K)
Total for K=100: ~3.5ms + 4µs = ~3.5ms
```

---

## 5. Neo4j-rs Integration

### Current State (Feb 2026)

```
Component          Status    LOC     What Works
─────────────────  ────────  ──────  ────────────────────────
Cypher Parser      Done      1,374   Full openCypher subset
Cypher Lexer       Done      435     58 token kinds
Cypher AST         Done      278     7 statement types
Planner            Working   436     18 logical operators
Executor           Working   1,171   Volcano pull model
Memory Backend     Working   624     CRUD + traversal
Model Types        Done      530     Node, Relationship, Path, Value
StorageBackend     Trait     ~100    Sacred contract (33 methods)
LadybugBackend     0 LOC     —       Feature-gated, not started
```

### StorageBackend Trait → BindSpace Mapping

The `StorageBackend` trait in `neo4j-rs/src/storage/mod.rs` maps directly to
BindSpace operations. No translation layer needed.

```
StorageBackend::create_node(labels, props)  → BindSpace.write(fingerprint) → Addr
StorageBackend::get_node(id)                → BindSpace.read(addr) → &BindNode
StorageBackend::create_relationship(...)    → BindSpace.link(from, verb, to)
StorageBackend::get_relationships(...)      → BindSpace.edges_out(addr)
StorageBackend::expand(start, rel, dir)     → BindSpace.traverse(from, verb)
StorageBackend::scan_nodes(label)           → Hamming scan on block_0
StorageBackend::find_by_property(...)       → XOR-bind property → scan
```

### LadybugBackend Implementation Plan

```rust
// neo4j-rs/src/storage/ladybug.rs

pub struct LadybugBackend {
    space: BindSpace,                        // THE storage
    id_to_addr: Vec<Option<Addr>>,           // NodeId → BindSpace Addr bridge
    addr_to_id: HashMap<Addr, NodeId>,       // Reverse lookup
    next_id: AtomicU64,                      // Sequential ID generator
}

#[async_trait]
impl StorageBackend for LadybugBackend {
    async fn create_node(&mut self, labels: &[&str], props: &PropertyMap) -> Result<NodeId> {
        // 1. Fingerprint labels + properties → Container (block_0)
        // 2. Encode properties as XOR-bound key-value pairs
        // 3. Write to BindSpace → get Addr
        // 4. Bridge NodeId ↔ Addr
        // 5. Set meta Q1 fields (dn_hash, flags, timestamps, NARS defaults)
        // 6. Return NodeId
    }
    // ... 32 more trait methods
}
```

### Cypher → RISC Compilation

The planner should compile Cypher patterns directly to RISC operations:

```
MATCH (a:Person)-[:KNOWS]->(b)
WHERE a.name = "Jan"
RETURN b.name

Compiles to:
  1. prop_fingerprint = BIND(codebook["name"], codebook[hash("Jan")])
  2. SCAN block_0 for nodes with label Person → candidate set A
  3. For each a in A:
       test = BIND(a.block_0, PERMUTE(prop_fingerprint, 1))
       if test EXISTS in store → a.name = "Jan"
  4. partial = BIND(a, PERMUTE(codebook["KNOWS"], 1))
  5. SCAN for edges matching partial within radius
  6. For each hit: b = UNBIND(edge, a, "KNOWS")
  7. Recover b.name via property unbind
```

### Phase Shifting (Post-MVP)

For point queries, verbs become rotation operators:

```
Old: SCAN for KNOWS edges from node_jan → O(N) scan
New: PERMUTE(jan.codebook, KNOWS_rotation) → CHECK alignment with target → O(1)
```

---

## 6. Proven Experiments (Reference Results)

### 6.1 cam_vision: Edge-Binding Image Recognition (Feb 21, 2026)

**What**: Render characters as 64×64 images, Sobel edge detection, quantize
orientations into 8 codebook tokens, XOR-bind top-2 orientations per 8×8
patch (the "X-crossing"), PERMUTE for spatial position, BUNDLE into one Word,
MATCH via Hamming distance.

**Results**: 4/5 correct on same-shape cross-rendering recognition (line-art
library → bitmap query). T and L achieve 0.68-0.72 similarity across renderings.
Confirms: XOR-bind of discrete codebook tokens produces discriminative fingerprints.

**Key code**: `/home/claude/cam_vision/src/main.rs` (416 lines, pure Rust, zero deps)

### 6.2 Random Hyperplane Projection Experiment (Feb 2026)

**What**: 100K real CLIP embeddings, tested structured vs random hyperplane
projection to binary.

**Results**: Random hyperplanes are optimal. Johnson-Lindenstrauss holds.
XOR-bind destroys signal when used as projection (pushes both intra and inter
toward 50%). **But**: XOR-bind works perfectly for composing discrete, already-
orthogonal codebook tokens. These are different operations:

- **Projection** (continuous → binary): Use random hyperplanes. JL is optimal.
- **Composition** (discrete tokens → compound features): Use XOR-bind. This is
  what edges, properties, and labels use. Not a projection.

**Implication**: Block 0 fingerprints should use random hyperplane projection
from CLIP/Jina embeddings. Block 2 stores the original int8 embedding for
exact reranking. XOR-bind is for composing codebook tokens (edges, properties),
not for projecting float vectors.

### 6.3 Ripple-Carry Bundle Benchmark (Feb 21, 2026)

**What**: Bit-parallel majority vote via ripple-carry counters, compared to
naive bit-by-bit iteration.

**Results**: 22-40× speedup. Zero correctness errors. The optimization
eliminates all 8192 per-bit branches, replacing them with u64-wide AND/XOR/OR
operations across 128 lanes.

**WARNING for ladybug-rs**: The ladybug `simd_ops` ripple-carry shows ~2.9ms
constant overhead due to heap allocation. Stack-allocate the counter array to
match cam_vision's 1.3µs performance. See §7.1.

---

## 7. Known Issues to Fix

### 7.1 ladybug-rs simd_ops: Bundle Allocation Overhead

**File**: `ladybug-rs/src/**/simd_ops/mod.rs` (or similar path)
**Problem**: `vec![[0u64; WORD_LANES]; nbits]` allocates on every call.
**Fix**: Use `[[u64; CONTAINER_LANES]; 16]` on the stack (supports up to 65535 inputs).
**Impact**: 2000× speedup for small N (2.9ms → 1.3µs).

### 7.2 ladybug-rs simd_ops: Arc::clone Temporary Value Issues

**File**: `simd_ops/mod.rs`
**Problem**: 15 instances of `Arc::clone(&x).lock().unwrap()` create temporaries
that don't live long enough.
**Fix**: Two-step pattern:
```rust
let binding = Arc::clone(&x);
let mut guard = binding.lock().unwrap();
```

### 7.3 ladybug-rs simd_ops: simd_popcount_u8x64 LUT Overflow

**File**: `simd_ops/mod.rs`
**Problem**: Manual LUT-based popcount on `u8x64` indexes beyond the 16-entry
table because nibble extraction overflows.
**Fix**: Replace with `count_ones()` per u64 lane. rustc emits native `POPCNT`
instruction when target supports it. No LUT needed.

```rust
// WRONG — LUT overflow
let lut = u8x64::from_array(LUT_16);
let lo = input & splat(0x0F);
let hi = (input >> 4) & splat(0x0F);
let pop = lut.swizzle(lo) + lut.swizzle(hi);  // ← overflows

// RIGHT — compiler emits POPCNT
fn distance(a: &Container, b: &Container) -> u32 {
    let mut d = 0u32;
    for i in 0..CONTAINER_LANES {
        d += (a.lanes[i] ^ b.lanes[i]).count_ones();
    }
    d
}
```

### 7.4 8 Failing Tests in ladybug-rs

165 tests pass, 8 fail. All failures are in hamming/popcount functions due to
the LUT issue above (§7.3). Fix the popcount → tests pass.

---

## 8. Implementation Roadmap

### Phase A: Container Upgrade (ladybug-rs)

**Goal**: Change Container from `[u64; 128]` to `[u64; 256]`.

1. Update `Container` struct in `ladybug-contract/src/container.rs`
2. Update `FINGERPRINT_WORDS` / `FINGERPRINT_BITS` constants
3. Update `MetaView` / `MetaViewMut` for new quadrant boundaries:
   - Q1: W0-W63 (was W0-W31) — CAM identity, doubled capacity
   - Q2: W64-W127 (was W32-W63) — 64 edge slots (was 32)
   - Q3: W128-W191 (was W64-W95) — 64 child refs (was 32)
   - Q4: W192-W255 (was W96-W127) — now usable (attention mask, phase cache)
4. Update CogRecord to use new Container size
5. Fix all `WORD_LANES` / `WORD_BITS` constants throughout codebase
6. Run `cargo test` — fix cascading type errors

**Estimated effort**: 2-4 hours. Mostly mechanical constant changes.

### Phase B: Fix SIMD Operations (ladybug-rs)

**Goal**: All 173 tests pass. Bundle at correct speed.

1. Fix `simd_popcount_u8x64` → use `count_ones()` per u64 (§7.3)
2. Fix 15 `Arc::clone` temporary value issues (§7.2)
3. Stack-allocate bundle counter array (§7.1)
4. Verify: `cargo test` → 173/173 pass
5. Verify: `cargo bench` → bundle matches cam_vision speedup (22-40×)

**Estimated effort**: 1-2 hours.

### Phase C: AVX-512 Intrinsics (ladybug-rs)

**Goal**: Native VPOPCNTDQ/VPXORQ for hot-path operations.

1. Add `#[target_feature(enable = "avx512f,avx512_vpopcntdq")]` to distance/bind
2. Use `core::arch::x86_64::*` intrinsics for the inner loops
3. Keep scalar fallback for non-AVX-512 targets (`cfg` feature gate)
4. Benchmark: distance should be ~35ns for 16384-bit Container

**Estimated effort**: 2-3 hours.

### Phase D: LadybugBackend (neo4j-rs)

**Goal**: `impl StorageBackend for LadybugBackend` — all Cypher works.

1. Create `neo4j-rs/src/storage/ladybug.rs`
2. Implement all 33 `StorageBackend` trait methods
3. Property encoding: XOR-bind key/value codebook tokens
4. Label encoding: XOR-bind label codebook tokens
5. Edge encoding: `src ⊕ permute(rel, 1) ⊕ permute(tgt, 2)`
6. All existing Memory Backend tests must pass on Ladybug Backend
7. Feature-gate: `#[cfg(feature = "ladybug")]`

**Estimated effort**: 8-16 hours (largest phase).

### Phase E: Embedding Store (ladybug-rs)

**Goal**: Block 2 holds int8 quantized embeddings with VNNI acceleration.

1. Define `EmbeddingMetadata` struct (model_id, scale, zero_point, norm)
2. Implement int8 quantization: f32 → int8 with scale/zero_point
3. Implement VNNI dot product: `VPDPBUSD` for int8 × int8
4. Two-phase search: Hamming filter (block_0) → VNNI rerank (block_2)
5. Benchmark: should match or beat FAISS IVF-PQ on same data

**Estimated effort**: 4-8 hours.

### Phase F: One-Binary Integration (holograph)

**Goal**: `cargo build --release` produces single `holograph` binary.

1. Create Cargo workspace with ladybug-rs + neo4j-rs as member crates
2. `src/main.rs` instantiates BindSpace + LadybugBackend + Cypher REPL
3. Enable LTO (`lto = "fat"`) for cross-crate inlining
4. Verify: SIMD operations inline across crate boundary
5. Blackboard borrow-mut pattern: readers get `&BindSpace`, writer gets `&mut`

**Estimated effort**: 2-4 hours.

---

## 9. Key Architecture Principles (Do Not Violate)

### From HOLOGRAPH_CONTRACT.md

1. **Content IS the address.** No indirection. No UUID→content lookup.
2. **One data type**: Container (`[u64; 256]`). Everything is a Container.
3. **Five operations**: BIND, BUNDLE, MATCH, PERMUTE, STORE/SCAN.
4. **Zero serialization** on the hot path. No JSON. No strings.
5. **The codebook IS the schema.** Adding a concept = adding a codebook entry.
6. **Edges ARE nodes.** W1[63] distinguishes them. Same Container.
7. **No HashMap side storage.** BindSpace is the only truth.

### From CLAUDE.md (neo4j-rs)

1. **StorageBackend trait is sacred.** All storage goes through it.
2. **Clean DTO boundary.** No Arrow/Lance/holograph types in neo4j-rs core.
3. **Parser owns nothing.** Pure function: `&str → Result<Statement>`.
4. **Memory Backend is the test oracle.** All Cypher works on Memory first.

### From This Session

1. **Random hyperplanes for projection, XOR-bind for composition.** Different domains.
2. **Stack-allocate the bundle counter.** Heap allocation kills performance.
3. **Use `count_ones()` for popcount.** Compiler emits POPCNT. No manual LUT.
4. **4×16384 bits.** The hardware supports it. Use it.

---

## 10. Files to Read Before Starting

In order:

1. This document (you're reading it)
2. `neo4j-rs/CLAUDE.md` — coding standards, architecture rules
3. `neo4j-rs/ARCHITECTURE.md` — crate structure, three-tier design
4. `neo4j-rs/src/storage/mod.rs` — the StorageBackend trait
5. `neo4j-rs/docs/INTEGRATION_PLAN_SCHEMA_CHANGES.md` — BindSpace mapping
6. `ada-docs/holograph/HOLOGRAPH_CONTRACT.md` — container spec (superseded by §2 above for size, but quadrant semantics are current)
7. `ada-docs/holograph/META_QUADRANTS.md` — Q1-Q4 field assignments
8. `ada-docs/architecture/CONTAINER0_CODEBOOK_IDENTITY.md` — codebook-as-identity philosophy

---

## 11. Quick Verification Commands

```bash
# Clone repos
PAT="$GITHUB_PAT (see ada-docs secure config)"
curl -sL -H "Authorization: token $PAT" \
  "https://api.github.com/repos/AdaWorldAPI/neo4j-rs/zipball/main" -o neo4j-rs.zip

# Check AVX-512 support
grep -o 'avx512[a-z_]*' /proc/cpuinfo | sort -u

# Run neo4j-rs tests
cd neo4j-rs && cargo test

# Run ladybug-rs tests (after cloning)
cd ladybug-rs && cargo test

# Run ladybug-rs benchmarks
cd ladybug-rs && cargo bench --bench hdc_benchmarks -- "Bundle"

# Build cam_vision reference implementation
cd cam_vision && cargo run --release
```

---

## 12. Success Criteria

| Metric | Target |
|--------|--------|
| Container size | 16,384 bits per container (256 × u64) |
| CogRecord size | 65,536 bits = 8 KB |
| Hamming distance (16K bits) | < 50ns |
| Bundle(64 words, 16K bits) | < 35µs |
| All ladybug-rs tests | 173/173 pass |
| All neo4j-rs tests | pass on LadybugBackend |
| One binary builds | `holograph` binary with LTO |
| Embedding search | Hamming pre-filter + VNNI rerank < 5ms for 1M records |

---

*This document was produced from a live development session on Feb 21, 2026,
where the ripple-carry bundle optimization was conceived, implemented, and
benchmarked at 22-40× speedup. The 4×16384 architecture was derived from
confirming AVX-512 VPOPCNTDQ hardware availability on the deployment target.*
