# Chess Knowledge Harvest Plan

Databases to ingest into neo4j-rs + ladybug-rs for the chess AI stack,
with bitpacked memory bridging chess logic and the AI War ontology.

---

## 1. Harvestable Databases

### A. Lichess Chess Openings (ECO)

| Field | Value |
|-------|-------|
| **Source** | [lichess-org/chess-openings](https://github.com/lichess-org/chess-openings) |
| **Format** | TSV + Apache Parquet |
| **Size** | ~3,500 named openings |
| **Fields** | ECO code, name, PGN moves, UCI moves, EPD (FEN) |
| **License** | CC0 |
| **Priority** | **P0 — harvest first** |

**Neo4j-rs mapping:**

```cypher
// Each opening is a node
CREATE (o:Opening {eco: "B90", name: "Sicilian, Najdorf", pgn: "1.e4 c5 2.Nf3 d6 3.d4 cxd4 4.Nxd4 Nf6 5.Nc3 a6"})

// Each position along the main line is a node
CREATE (p:Position {fen: "rnbqkb1r/1p2pppp/p2p1n2/8/3NP3/2N5/PPP2PPP/R1BQKB1R w KQkq - 0 6"})

// Link positions in sequence
(p1:Position)-[:MOVE {san: "e4", eco: "B90"}]->(p2:Position)

// Link positions to their opening classification
(p:Position)-[:BELONGS_TO]->(o:Opening)

// Opening family tree
(o1:Opening {eco: "B90"})-[:VARIATION_OF]->(o2:Opening {eco: "B80"})
```

**Ladybug fingerprint:** Each ECO opening gets a fingerprint encoding its strategic character (open/closed, pawn structure, piece activity pattern). RESONATE finds "openings similar to the Najdorf."

---

### B. eco.json (Extended Opening Variations)

| Field | Value |
|-------|-------|
| **Source** | [hayatbiralem/eco.json](https://github.com/hayatbiralem/eco.json) |
| **Format** | JSON (FEN-keyed) |
| **Size** | 12,000+ variations |
| **Fields** | FEN → {eco, moves, name, scid}, plus `fromTo.json` transition graph |
| **License** | Public |
| **Priority** | **P0 — harvest with openings** |

**Key value:** The `fromTo.json` file is a **raw transition graph** — exactly what neo4j-rs is built for. Each FEN is a node, each move is an edge. Direct ingest into `Graph<MemoryBackend>`.

---

### C. Lichess Evaluation Database

| Field | Value |
|-------|-------|
| **Source** | [database.lichess.org/eval](https://database.lichess.org/#evals) |
| **Format** | JSONL (one position per line) |
| **Size** | 302,517,109 positions |
| **Fields** | FEN, depth, knodes, principal variations (PVs) with cp/mate eval |
| **License** | CC0 |
| **Priority** | **P1 — sample, don't ingest all 302M** |

**Strategy:** Sample positions by depth threshold (depth ≥ 40 = high-quality evals) and by opening coverage. Target ~5M positions for the initial graph.

**Neo4j-rs mapping:**

```cypher
CREATE (p:Position {fen: $fen, eval_cp: 35, depth: 46, knodes: 128000})

// Store principal variations as move chains
(p)-[:PV {rank: 1, eval_cp: 35}]->(m1:Move {uci: "e2e4"})-[:THEN]->(m2:Move {uci: "c7c5"})

// Cross-reference with opening positions
MATCH (p:Position)-[:BELONGS_TO]->(o:Opening)
SET p.eval_cp = 35, p.sf_depth = 46
```

**Ladybug fingerprint:** Each evaluated position gets a fingerprint. The eval score becomes metadata on the fingerprint. RESONATE("find positions similar to this where eval > +2.0") = find winning plans.

---

### D. Syzygy Endgame Tablebases

| Field | Value |
|-------|-------|
| **Source** | [syzygy-tables.info](https://syzygy-tables.info/) |
| **Format** | Binary `.rtbw` (WDL) + `.rtbz` (DTZ) |
| **Size** | 7-piece: ~140 TB uncompressed, 3-4-5-piece: ~1 GB |
| **Fields** | Position → Win/Draw/Loss + Distance-to-Zeroing |
| **License** | GPL |
| **Priority** | **P2 — 3-4-5 piece only** |

**Strategy:** Don't ingest the raw binary tables. Instead, harvest the **boundary positions** (positions where the evaluation flips from win to draw or draw to loss) — these are the most instructive for the Endgame Specialist agent.

**Neo4j-rs mapping:**

```cypher
CREATE (p:Position:Endgame {fen: $fen, pieces: 5, wdl: "win", dtz: 23})

// Boundary positions are especially valuable
(p:Position {wdl: "win"})-[:MOVE {san: "Kd5", wdl_change: "win→draw"}]->(q:Position {wdl: "draw"})

// Link to endgame classification
(p)-[:ENDGAME_TYPE]->(e:EndgameType {name: "KRvKB", theoretical: "win"})
```

---

### E. Polyglot Opening Books

| Field | Value |
|-------|-------|
| **Source** | Various (e.g., [Polyfish](https://github.com/khalid-a-omar/Polyfish)) |
| **Format** | Binary `.bin` (16 bytes per entry: hash + move + weight + learn) |
| **Size** | 10-100 MB per book |
| **Fields** | Zobrist hash → move, weight, win/draw/loss stats |
| **License** | Varies |
| **Priority** | **P1 — complements ECO data with move frequencies** |

**Key value:** Polyglot books contain **move weights** (how often GMs play each move) and **learning data** (Stockfish score for each move). This is exactly what the Strategist agent needs for the opening book graph.

**Neo4j-rs mapping:**

```cypher
// Each book entry becomes an edge with weight
(p:Position {zobrist: $hash})-[:BOOK_MOVE {
    san: $san,
    weight: 12450,        // frequency weight
    learn: 35,            // Stockfish eval in cp
    win_pct: 0.54,
    draw_pct: 0.31,
    loss_pct: 0.15
}]->(q:Position)
```

---

### F. Lichess Game Database (Sampled)

| Field | Value |
|-------|-------|
| **Source** | [database.lichess.org](https://database.lichess.org/) |
| **Format** | PGN (zst compressed) |
| **Size** | 7.2 billion games, ~2 TB compressed |
| **Fields** | Moves, result, Elo ratings, time control, clock, eval annotations |
| **License** | CC0 |
| **Priority** | **P2 — sample elite games only** |

**Strategy:** Use the [Lichess Elite Database](https://database.nikonoel.fr/) (games where both players rated 2400+) as a manageable subset. ~5M games, ~200M positions.

**Neo4j-rs mapping:**

```cypher
CREATE (g:Game {id: $id, result: "1-0", white_elo: 2650, black_elo: 2580, date: date("2024-03-15")})

// Game → Position chain
(g)-[:MOVE_1]->(p1:Position)-[:MOVE {san: "e4"}]->(p2:Position)-[:MOVE {san: "c5"}]->...

// Player nodes (optional, for opponent modeling)
(player:Player {name: "Carlsen"})-[:PLAYED_WHITE]->(g)
```

---

## 2. Bitpacked Memory: Chess Logic as Cognitive Architecture

This is where ladybug-rs transforms chess from "database lookup" into **cognitive resonance**.

### Chess Position → 16,384-bit Fingerprint

A chess position is already bitboard-native. Standard engines use 12 bitboards × 64 bits = 768 bits. Ladybug extends this into a full cognitive fingerprint:

```
┌──────────────────────────────────────────────────────────────────┐
│              Position Fingerprint (16,384 bits)                   │
│                                                                   │
│  Bits 0-767:     Raw Bitboards (12 piece types × 64 squares)     │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 768-1023:  Pawn Structure Features                          │
│    • Doubled pawns (per file, 8 bits × 2 colors)                  │
│    • Isolated pawns (per file, 8 bits × 2 colors)                 │
│    • Passed pawns (per file, 8 bits × 2 colors)                   │
│    • Pawn chains (direction-encoded, 16 bits × 2 colors)          │
│    • Pawn islands (count-encoded, 4 bits × 2 colors)              │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 1024-1279: King Safety                                      │
│    • Pawn shield (3 squares × 2 sides × 8 configs = 48 bits)      │
│    • Open files near king (8 bits × 2 sides)                      │
│    • Attack vectors to king (8 directions × 4 range × 2 = 64 bits)│
│    • Castling rights + has-castled (4 + 2 bits)                   │
│    • King tropism (piece proximity encoded, 64 bits × 2 sides)    │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 1280-1791: Piece Activity                                   │
│    • Mobility per piece (legal moves count, 7 bits × 32 pieces)   │
│    • Centralization score (distance-from-center, 3 bits × 32)     │
│    • Coordination (piece pairs defending each other, 64 bits)     │
│    • Outpost squares (knight/bishop, 64 bits)                     │
│    • Rook on open/semi-open file (16 bits)                        │
│    • Bishop pair flag (1 bit × 2 sides)                           │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 1792-2303: Tactical Motifs                                  │
│    • Pins (absolute + relative, 64 bits × 2)                      │
│    • Forks (knight/bishop/pawn, 64 bits × 3)                      │
│    • Skewers (rook/queen/bishop, 64 bits × 3)                     │
│    • Discovered attack potential (64 bits)                         │
│    • X-ray attacks (64 bits)                                      │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 2304-2559: Material Signature                               │
│    • Material balance (encoded as surplus per piece type, 48 bits) │
│    • Material hash (imbalance signature, 64 bits)                 │
│    • Total material (phase score, 8 bits × 2)                     │
│    • Endgame type fingerprint (KRvKB etc., 64 bits)               │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 2560-4095: Strategic Themes                                 │
│    • Space advantage (controlled squares, 64 bits × 2)            │
│    • Pawn majority location (kingside/center/queenside, 6 bits)   │
│    • Color complex weakness (light/dark square control, 64 bits)  │
│    • Piece placement pattern (hashed, 256 bits)                   │
│    • Tempo/initiative markers (64 bits)                           │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 4096-8191: Opening/Plan Context                             │
│    • ECO family embedding (one-hot over 500 codes, 512 bits)      │
│    • Move history hash (last 10 moves, 640 bits)                  │
│    • Plan theme embedding (attack/defense/transition, 256 bits)   │
│    • Game phase (opening/middle/endgame, continuous, 32 bits)     │
│    • Reserved for agent annotations (2,656 bits)                  │
│  ────────────────────────────────────────────────────────────────  │
│  Bits 8192-16383: AI War Cognitive Bridge                         │
│    • AIRO risk dimensions (mapped from aiwar ontology, 1024 bits) │
│    • Agent decision trace hash (512 bits)                         │
│    • Similarity cluster ID (HDR cascade levels, 256 bits)         │
│    • Reserved for cross-domain resonance (6,400 bits)             │
│                                                                   │
│    This is where chess positions become "AI systems" in the        │
│    AI War ontology. A position with an unstoppable passed pawn    │
│    RESONATES with an AI system with unstoppable capabilities.     │
│    A king in danger RESONATES with a vulnerable infrastructure.   │
│    The same CAM operations work on both domains.                  │
└──────────────────────────────────────────────────────────────────┘
```

### HDR Cascade for Chess

```
Level 0 (1-bit):    Open vs Closed position
Level 1 (4-bit):    Pawn structure family (16 types)
Level 2 (8-bit):    Tactical theme cluster (256 types)
Level 3 (16-bit):   Strategic pattern group (65K types)
Level 4 (full):     Exact Hamming distance (16,384-bit)

Search path:
  "Find positions similar to this Sicilian Najdorf"
  → L0: Open position → filter 60% of database
  → L1: e-pawn vs c-pawn structure → filter to 8%
  → L2: Kingside attack cluster → filter to 0.5%
  → L3: Najdorf-family patterns → filter to 0.02%
  → L4: Hamming top-10 → exact matches
```

### BindSpace Addressing for Chess

```
ladybug-rs BindSpace: 8-bit prefix + 8-bit slot = 16-bit address

Chess address scheme:
  Prefix 0x00-0x0B: Piece bitboards (12 piece types)
  Prefix 0x0C:      Pawn structure features
  Prefix 0x0D:      King safety features
  Prefix 0x0E:      Piece activity features
  Prefix 0x0F:      Tactical motifs
  Prefix 0x10:      Material signature
  Prefix 0x11-0x13: Strategic themes
  Prefix 0x14-0x1F: Opening/plan context
  Prefix 0x20-0x3F: AI War cognitive bridge
  Prefix 0x40-0x7F: Agent decision memory (per-agent slots)
  Prefix 0x80-0xFF: Reserved / temporal snapshots

Each "slot" within a prefix = specific feature dimension
  e.g., Prefix 0x0D (King Safety), Slot 0x03 = white king pawn shield
```

### CAM Operations: Chess Meets AI War

The same cognitive operations work on both chess positions and AI War systems:

```
RESONATE(position_fp, k=10)
  Chess: Find 10 most similar positions in the opening book
  AIWar: Find 10 most similar AI systems by capability profile
  BRIDGE: A chess position with overwhelming piece activity RESONATES
          with an AI system that has overwhelming capability concentration

SUPERPOSE(white_plan_fp, black_plan_fp)
  Chess: Merge both sides' strategic intentions into a tension fingerprint
  AIWar: Merge attacker + defender capability profiles
  BRIDGE: Strategic tension in chess = competitive pressure in AI landscape

INHIBIT(candidate_move_fp, known_blunder_fp)
  Chess: Suppress move candidates that match known blunder patterns
  AIWar: Suppress deployment options that match known failure patterns
  BRIDGE: "Don't play into a trap" = "Don't deploy into a known vulnerability"

XOR(position_before, position_after)
  Chess: Measure how much a single move changed the position character
  AIWar: Measure how much a system update changed its risk profile
  BRIDGE: A quiet positional move = incremental capability update
          A sacrificial combination = disruptive capability leap

BIND(position_fp, agent_decision_fp)
  Chess: Associate a position with the agent's reasoning about it
  AIWar: Associate a system state with the analysis that evaluated it
  BRIDGE: Creates persistent memory — "last time I saw this pattern,
          I decided X, and it worked/failed"
```

---

## 3. The Bridge: Chess Positions ARE AI Systems

The deep insight is that chess positions and AI War systems share **the same abstract structure** when viewed through ladybug's cognitive lens:

| Chess Concept | AI War Concept | Shared Fingerprint Dimension |
|---------------|----------------|------------------------------|
| Material (pieces) | Capabilities (features) | Resource inventory |
| Pawn structure | Infrastructure | Long-term positional assets |
| King safety | Vulnerability surface | Defensive posture |
| Piece activity | Operational tempo | How effectively resources are deployed |
| Tactical threats | Attack vectors | Immediate forcing possibilities |
| Strategic plan | Deployment strategy | Long-term objective pursuit |
| Game phase | System maturity | Lifecycle stage |
| Opening theory | Known patterns | Established knowledge base |
| Endgame technique | Capability conversion | Converting advantages to outcomes |
| Time pressure | Decision latency | Resource constraints on reasoning |

**An agent trained on chess positions can RESONATE with AI War systems because the fingerprint encoding captures the same abstract patterns.** A position where White has a crushing kingside attack but an exposed queen is structurally similar to an AI system with powerful offensive capabilities but a single critical vulnerability.

This is not metaphor — it's **the same Hamming distance computation** over the same bit dimensions, with the AI War bridge occupying bits 8192-16383 of the fingerprint.

---

## 4. Ingest Pipeline

### Phase 1: Opening Knowledge (Day 1)

```
1. Clone lichess-org/chess-openings
2. Clone hayatbiralem/eco.json
3. Parse TSV + JSON into neo4j-rs CREATE statements
4. Build opening graph: 3,500+ openings, 12,000+ variations
5. Compute ladybug fingerprints for each named position
6. Store fingerprints in BindSpace (prefix 0x14-0x1F)
7. Build HDR cascade index for fast similarity search
```

**Estimated graph:** ~15,000 nodes (positions + openings), ~25,000 edges (moves + classifications)

### Phase 2: Evaluation Layer (Week 1)

```
1. Download Lichess eval database (JSONL)
2. Filter: depth ≥ 40, keep ~5M positions
3. Match against opening positions (merge, don't duplicate)
4. Attach eval_cp, depth, PVs as properties
5. For un-opened positions: create new Position nodes
6. Compute fingerprints for all new positions
7. Link evaluated positions to similar positions via SIMILAR_TO edges
```

**Estimated graph:** ~5M nodes, ~20M edges (moves + similarity + PVs)

### Phase 3: Endgame Truth (Week 2)

```
1. Download 3-4-5 piece Syzygy tables (~1 GB)
2. Extract boundary positions (WDL transitions)
3. Create EndgameType classification nodes
4. Build WDL/DTZ property edges
5. Fingerprint endgame positions
6. Cross-reference: which openings lead to which endgames?
```

**Estimated graph addition:** ~500K boundary positions, ~2M edges

### Phase 4: Elite Games (Week 3)

```
1. Download Lichess Elite Database (2400+ rated)
2. Parse PGN, extract unique positions + move frequencies
3. Create Player and Game nodes
4. Build game-level position chains
5. Compute per-player style fingerprints
6. Feed into Psychologist agent's opponent modeling
```

**Estimated graph addition:** ~50M positions (deduplicated), ~200M edges

### Phase 5: AI War Bridge (Week 4)

```
1. Map aiwar-neo4j-harvest's 221 AI system nodes to chess analogues
2. Compute cross-domain fingerprints (bits 8192-16383)
3. Build RESONATES_WITH edges between chess positions and AI systems
4. Test CAM operations across domains
5. Validate: does a "crushing attack" position actually resonate
   with a "dominant capability" AI system?
```

---

## 5. Tooling: aiwar-neo4j-harvest as the Harvester

The existing aiwar-neo4j-harvest CLI can be extended with new subcommands:

```
# Current
aiwar-neo4j-harvest cypher    # Interactive Cypher shell
aiwar-neo4j-harvest neo4j     # Ingest AI War data
aiwar-neo4j-harvest analyze   # Run analysis patterns

# New
aiwar-neo4j-harvest chess-openings   # Ingest ECO + eco.json
aiwar-neo4j-harvest chess-evals      # Ingest Lichess evaluations
aiwar-neo4j-harvest chess-endgames   # Ingest Syzygy boundary positions
aiwar-neo4j-harvest chess-games      # Ingest elite PGN games
aiwar-neo4j-harvest chess-bridge     # Compute cross-domain fingerprints
aiwar-neo4j-harvest chess-all        # Full pipeline (phases 1-5)
```

Each subcommand:
1. Downloads the source data (with caching)
2. Parses into neo4j-rs CREATE/MERGE statements
3. Computes ladybug fingerprints
4. Ingests into `Graph<MemoryBackend>` or `Graph<LadybugBackend>`
5. Reports statistics (nodes, edges, fingerprints, cascade levels)

---

## 6. Dependencies Needed

```toml
# In aiwar-neo4j-harvest/Cargo.toml (additions)
shakmaty = "0.27"          # Rust chess library (bitboard, move gen, FEN/PGN)
pgn-reader = "0.26"        # Fast PGN parser
polyglot = "0.2"           # Polyglot opening book reader (or custom)
# neo4j-rs = { path = "../neo4j-rs" }  # already planned
# ladybug = { path = "../ladybug-rs" } # via neo4j-rs feature
```

[shakmaty](https://crates.io/crates/shakmaty) is the gold-standard Rust chess library. It provides bitboard representations, legal move generation, and FEN/SAN/UCI parsing — the exact primitives needed for fingerprint computation.

---

## Sources

- [Lichess Chess Openings](https://github.com/lichess-org/chess-openings) — TSV/Parquet ECO database (CC0)
- [eco.json](https://github.com/hayatbiralem/eco.json) — 12K+ FEN-keyed opening variations with transition graph
- [Lichess Evaluation Database](https://database.lichess.org/#evals) — 302M positions with Stockfish evals (CC0)
- [Syzygy Endgame Tablebases](https://www.chessprogramming.org/Syzygy_Bases) — 7-piece WDL/DTZ truth tables
- [Polyglot Opening Books](https://www.chessprogramming.org/Opening_Book) — Binary format, hash → move + weight
- [Lichess Game Database](https://database.lichess.org/) — 7.2B games in PGN (CC0)
- [Lichess Elite Database](https://database.nikonoel.fr/) — 2400+ rated player games
- [Stockfish NNUE Architecture](https://official-stockfish.github.io/docs/nnue-pytorch-wiki/docs/nnue.html) — HalfKA features, training data format
- [Polyfish](https://github.com/khalid-a-omar/Polyfish) — Stockfish + Polyglot/CTG support
- [shakmaty](https://crates.io/crates/shakmaty) — Rust chess library for bitboard operations
