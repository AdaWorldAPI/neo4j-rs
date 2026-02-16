# GUI Plan — neo4j-rs Graph Explorer (egui/eframe)

> **Updated**: 2026-02-16
> **Crate**: `neo4j-rs-gui/` (separate binary, depends on `neo4j-rs`)
> **Stack**: egui 0.29 + eframe + egui_graphs + egui_code_editor
> **Rule**: GUI depends on neo4j-rs with ladybug feature, never the reverse

---

## 1. Crate Layout

```
neo4j-rs-gui/                ← new binary crate
├── Cargo.toml
├── src/
│   ├── main.rs              ← eframe::run_native()
│   ├── app.rs               ← App struct, impl eframe::App
│   ├── panels/
│   │   ├── cypher_editor.rs     ← Monaco-like code editor
│   │   ├── graph_viewer.rs      ← Force-directed graph viz (egui_graphs)
│   │   ├── result_table.rs      ← QueryResult → table view
│   │   ├── node_inspector.rs    ← Selected node detail + CogRecord view
│   │   ├── cam_browser.rs       ← CAM op catalog, filter by namespace
│   │   └── qualia_heatmap.rs    ← 7-layer qualia visualization
│   ├── backend.rs           ← Async bridge to LadybugBackend
│   └── state.rs             ← App state, query history
```

---

## 2. Dependencies

```toml
[dependencies]
neo4j-rs = { path = "../neo4j-rs", features = ["ladybug"] }
eframe = "0.29"
egui = "0.29"
egui_extras = "0.29"             # TableBuilder for result tables
egui_graphs = "0.22"             # Force-directed graph visualization
egui_code_editor = "0.3"         # Syntax-highlighted Cypher editor
tokio = { version = "1", features = ["full"] }
```

---

## 3. Panel Layout

```
┌─────────────────────────────────────────────────────────────────┐
│  [Cypher Editor]                         │ [CAM Op Browser]     │
│  MATCH (n:Thought)-[:CAUSES]->(m)        │ 0x200 MatchNode      │
│  WHERE n.content CONTAINS 'Ada'          │ 0x201 MatchEdge      │
│  RETURN n, m                             │ 0x220 CreateNode     │
│  [▶ Execute]  [Explain Plan]             │ 0x260 ShortestPath   │
├──────────────────────────────────────────┤ ...                  │
│  [Graph Visualization]                    │                      │
│                                          │ Filter: [Cypher ▼]   │
│   (Ada)──CAUSES──>(Memory)               │                      │
│     │                  │                 ├──────────────────────┤
│     └──SUPPORTS──>(Felt)                 │ [Node Inspector]     │
│                                          │ DN: /0/3/7           │
│  [zoom] [pan] [layout: force/tree/ring]  │ Labels: Thought      │
├──────────────────────────────────────────┤ Rung: R3 Analogical  │
│  [Result Table]                          │ NARS: f=0.82 c=0.67  │
│  ┌──────┬──────────┬──────────┐         │ Edges: 12 inline     │
│  │ n    │ m        │ rel      │         │ Popcount: 4211       │
│  ├──────┼──────────┼──────────┤         │                      │
│  │ Ada  │ Memory   │ CAUSES   │         │ [Qualia Heatmap]     │
│  │ Ada  │ Felt     │ SUPPORTS │         │ texture: ████░░ 0.67 │
│  └──────┴──────────┴──────────┘         │ warmth:  █████░ 0.82 │
│                                          │ depth:   ███░░░ 0.45 │
│  [Explain Plan] [Export CSV]             │ entropy: ██░░░░ 0.31 │
└──────────────────────────────────────────┴──────────────────────┘
```

---

## 4. Graph Viewer — Node/Edge Visual Encoding

Each node carries CogRecord metadata that drives visual properties:

```rust
/// Graph node with CogRecord-derived visuals
struct GraphNode {
    id: NodeId,
    label: String,
    dn: PackedDn,
    // Visual properties derived from CogRecord:
    size: f32,      // from popcount (information content)
    color: Color32, // from rung level (R0=blue → R9=gold)
    glow: f32,      // from NARS confidence (brighter = more certain)
    pulse: f32,     // from arousal (Layer 5 cognition marker)
}

/// Graph edge with verb-based visual encoding
struct GraphEdge {
    verb: String,
    weight: f32,    // from CAM edge weight
    hamming: u32,   // Hamming distance between src/dst containers
    // Visual:
    thickness: f32, // from weight
    opacity: f32,   // from hamming (closer = more opaque)
    color: Color32, // from verb type (CAUSES=red, SUPPORTS=green, ...)
}
```

---

## 5. Cypher Editor — Syntax Highlighting + Autocomplete

```rust
const CYPHER_KEYWORDS: &[&str] = &[
    "MATCH", "WHERE", "RETURN", "CREATE", "MERGE", "DELETE",
    "SET", "REMOVE", "WITH", "UNWIND", "OPTIONAL", "DETACH",
    "ORDER", "BY", "SKIP", "LIMIT", "DISTINCT", "CASE",
    "WHEN", "THEN", "ELSE", "END", "AND", "OR", "NOT",
    "AS", "IN", "IS", "NULL", "TRUE", "FALSE",
];

// Auto-complete sources:
// 1. Labels in current graph      (from StorageBackend::all_labels())
// 2. Relationship types            (from StorageBackend::all_relationship_types())
// 3. CAM op procedures             (CALL ladybug.hamming_knn(...))
// 4. Property names                (from schema inspection)
```

---

## 6. CAM Op Browser

Interactive catalog of all 4096 CAM operations, filterable by namespace:

```rust
struct CamBrowser {
    filter_namespace: Option<OpCategory>,  // Cypher, Hamming, NARS, etc.
    search_text: String,
    selected_op: Option<u16>,              // CAM address
}

// Namespaces:
// 0x000-0x0FF  Core container ops
// 0x100-0x1FF  Edge/relationship verbs
// 0x200-0x2FF  Cypher operations      ← primary focus
// 0x300-0x3FF  NARS inference ops
// 0x400-0x4FF  Qualia stack ops
// ...

// Click an op → insert into Cypher editor as:
//   CALL ladybug.cam(0x200, {node_fp: $fp}) YIELD result
```

---

## 7. Qualia Heatmap

Visualize the 7-layer qualia stack for the selected node:

```rust
fn draw_qualia_heatmap(ui: &mut Ui, record: &CogRecord) {
    // Layer 1: Texture (8 dimensions)
    //   entropy, purity, density, bridgeness, warmth, edge, depth, flow
    // Layer 2: Meaning Axes (48 bipolar dimensions, 8 families)
    //   Osgood EPA, physical, spatiotemporal, cognitive, emotional, social, abstract, sensory
    // Layer 3: Resonance (HDR cascade visualization)
    // Layer 4: Gestalt (I/Thou/It triangle)
    // Layer 5: Council (Guardian/Catalyst/Balanced votes)
    // Layer 6: Felt Traversal (surprise at each branch)
    // Layer 7: Reflection (REVISE/CONFIRM/EXPLORE/STABLE)

    for layer in 0..7 {
        let values = extract_layer_values(record, layer);
        ui.horizontal(|ui| {
            ui.label(LAYER_NAMES[layer]);
            for (name, val) in values {
                let color = value_to_color(val); // blue→red gradient
                ui.colored_label(color, format!("{}: {:.2}", name, val));
            }
        });
    }
}
```

---

## 8. Async Backend Bridge

The GUI runs on the eframe main thread; storage calls are async. Bridge via
a tokio channel:

```rust
struct BackendBridge {
    /// Send queries from UI thread → tokio runtime
    query_tx: mpsc::Sender<QueryRequest>,
    /// Receive results from tokio runtime → UI thread
    result_rx: mpsc::Receiver<QueryResult>,
    /// Tokio runtime handle
    rt: tokio::runtime::Runtime,
}

impl BackendBridge {
    fn new(backend: LadybugBackend) -> Self {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (query_tx, query_rx) = mpsc::channel(32);
        let (result_tx, result_rx) = mpsc::channel(32);

        rt.spawn(async move {
            let graph = Graph::with_backend(backend);
            while let Some(req) = query_rx.recv().await {
                let result = graph.execute(&req.cypher, req.params).await;
                result_tx.send(result).await.ok();
            }
        });

        Self { query_tx, result_rx, rt }
    }
}
```

---

## 9. Build Order

### Phase B — GUI Skeleton (can start now)

| # | Task | Est. LOC |
|---|------|:--------:|
| B.1 | Create `neo4j-rs-gui` binary crate with eframe boilerplate | ~100 |
| B.2 | Cypher editor panel (egui_code_editor with Cypher syntax) | ~200 |
| B.3 | Result table panel (egui_extras::TableBuilder) | ~150 |
| B.4 | Wire to MemoryBackend via async channel | ~200 |

### Phase C — Graph Visualization

| # | Task | Est. LOC |
|---|------|:--------:|
| C.1 | Integrate egui_graphs for force-directed layout | ~300 |
| C.2 | Map CogRecord metadata → visual node properties | ~150 |
| C.3 | Interactive: click node → Node Inspector panel | ~200 |
| C.4 | Edge rendering with verb-based coloring | ~150 |

### Phase D — Advanced Panels

| # | Task | Est. LOC |
|---|------|:--------:|
| D.1 | CAM Op Browser (catalog of 0x000–0xFFF) | ~300 |
| D.2 | Qualia Heatmap (7-layer visualization) | ~250 |
| D.3 | DN-Tree navigator (hierarchical address space browser) | ~200 |
| D.4 | Explain Plan view (LogicalPlan → physical execution steps) | ~200 |

---

*The GUI is Phase 5 in the overall roadmap but can begin development immediately
against MemoryBackend. Switch to LadybugBackend when Phase 4 is complete — the
StorageBackend trait means zero GUI code changes required.*
