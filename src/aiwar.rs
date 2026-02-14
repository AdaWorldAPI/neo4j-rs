//! AI War tactical graph + chess knowledge graph for neo4j-rs.
//!
//! Populates the property graph with chess positions, openings, and moves,
//! bridging to the AI War tactical simulation layer.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                    AI WAR × CHESS GRAPH                        │
//! ├────────────────────────────────────────────────────────────────┤
//! │                                                                │
//! │  (:Position {fen, eval_cp, phase})                             │
//! │       -[:PLAYS_TO {uci, san}]->                                │
//! │  (:Opening {eco, name, moves})                                 │
//! │       -[:STARTS_AT]->(:Position)                               │
//! │  (:TacticalConcept {name, domain})                             │
//! │       -[:MAPS_TO]->(:TacticalConcept)                          │
//! │  (:System {name, type, status})     ← AI War nodes             │
//! │       -[:DEVELOPED_BY]->(:Stakeholder)                         │
//! │                                                                │
//! └────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Uses real stonksfish evaluation for position nodes and ladybug-rs
//! fingerprints (when feature-enabled) for similarity search.

use std::collections::HashMap;
use std::str::FromStr;

use chess::Board;

use crate::model::{NodeId, PropertyMap, Value};
use crate::storage::{MemoryBackend, StorageBackend};
use crate::tx::TxMode;
use crate::{Graph, Result};

// ============================================================================
// Core opening data: ECO classification
// ============================================================================

/// A chess opening entry for graph population.
pub struct OpeningEntry {
    pub eco: &'static str,
    pub name: &'static str,
    pub pgn: &'static str,
    pub fen: &'static str,
}

/// Built-in opening database (seed set from aiwar-neo4j-harvest).
/// Full database is populated from `cargo run -- chess-openings` data.
pub fn seed_openings() -> Vec<OpeningEntry> {
    vec![
        OpeningEntry { eco: "B20", name: "Sicilian Defense", pgn: "1. e4 c5", fen: "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2" },
        OpeningEntry { eco: "C00", name: "French Defense", pgn: "1. e4 e6", fen: "rnbqkbnr/pppp1ppp/4p3/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2" },
        OpeningEntry { eco: "B10", name: "Caro-Kann Defense", pgn: "1. e4 c6", fen: "rnbqkbnr/pp1ppppp/2p5/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2" },
        OpeningEntry { eco: "C50", name: "Italian Game", pgn: "1. e4 e5 2. Nf3 Nc6 3. Bc4", fen: "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3" },
        OpeningEntry { eco: "C60", name: "Ruy Lopez", pgn: "1. e4 e5 2. Nf3 Nc6 3. Bb5", fen: "r1bqkbnr/pppp1ppp/2n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 3 3" },
        OpeningEntry { eco: "D06", name: "Queen's Gambit", pgn: "1. d4 d5 2. c4", fen: "rnbqkbnr/ppp1pppp/8/3p4/2PP4/8/PP2PPPP/RNBQKBNR b KQkq c3 0 2" },
        OpeningEntry { eco: "E60", name: "King's Indian Defense", pgn: "1. d4 Nf6 2. c4 g6", fen: "rnbqkb1r/pppppp1p/5np1/8/2PP4/8/PP2PPPP/RNBQKBNR w KQkq - 0 3" },
        OpeningEntry { eco: "A10", name: "English Opening", pgn: "1. c4", fen: "rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR b KQkq c3 0 1" },
        OpeningEntry { eco: "B07", name: "Pirc Defense", pgn: "1. e4 d6 2. d4 Nf6", fen: "rnbqkb1r/ppp1pppp/3p1n2/8/3PP3/8/PPP2PPP/RNBQKBNR w KQkq - 1 3" },
        OpeningEntry { eco: "B01", name: "Scandinavian Defense", pgn: "1. e4 d5", fen: "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2" },
        OpeningEntry { eco: "B02", name: "Alekhine's Defense", pgn: "1. e4 Nf6", fen: "rnbqkb1r/pppppppp/5n2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2" },
        OpeningEntry { eco: "A80", name: "Dutch Defense", pgn: "1. d4 f5", fen: "rnbqkbnr/ppppp1pp/8/5p2/3P4/8/PPP1PPPP/RNBQKBNR w KQkq f6 0 2" },
        OpeningEntry { eco: "C42", name: "Petrov's Defense", pgn: "1. e4 e5 2. Nf3 Nf6", fen: "rnbqkb1r/pppp1ppp/5n2/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3" },
        OpeningEntry { eco: "D35", name: "Queen's Gambit Declined", pgn: "1. d4 d5 2. c4 e6", fen: "rnbqkbnr/ppp2ppp/4p3/3p4/2PP4/8/PP2PPPP/RNBQKBNR w KQkq - 0 3" },
        OpeningEntry { eco: "A45", name: "Trompowsky Attack", pgn: "1. d4 Nf6 2. Bg5", fen: "rnbqkb1r/pppppppp/5n2/6B1/3P4/8/PPP1PPPP/RN1QKBNR b KQkq - 2 2" },
        OpeningEntry { eco: "B06", name: "Modern Defense", pgn: "1. e4 g6", fen: "rnbqkbnr/pppppp1p/6p1/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2" },
        OpeningEntry { eco: "A00", name: "Uncommon Opening", pgn: "1. g4", fen: "rnbqkbnr/pppppppp/8/8/6P1/8/PPPPPP1P/RNBQKBNR b KQkq g3 0 1" },
        OpeningEntry { eco: "C44", name: "Scotch Game", pgn: "1. e4 e5 2. Nf3 Nc6 3. d4", fen: "r1bqkbnr/pppp1ppp/2n5/4p3/3PP3/5N2/PPP2PPP/RNBQKB1R b KQkq d3 0 3" },
        OpeningEntry { eco: "D20", name: "Queen's Gambit Accepted", pgn: "1. d4 d5 2. c4 dxc4", fen: "rnbqkbnr/ppp1pppp/8/8/2pP4/8/PP2PPPP/RNBQKBNR w KQkq - 0 3" },
        OpeningEntry { eco: "E00", name: "Catalan Opening", pgn: "1. d4 Nf6 2. c4 e6 3. g3", fen: "rnbqkb1r/pppp1ppp/4pn2/8/2PP4/6P1/PP2PP1P/RNBQKBNR b KQkq - 0 3" },
    ]
}

// ============================================================================
// AI War tactical bridge concepts
// ============================================================================

/// Chess ↔ AI War cross-domain bridge mappings.
pub fn tactical_bridge() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("Material", "Capabilities", "resource_inventory"),
        ("PawnStructure", "Infrastructure", "positional_assets"),
        ("KingSafety", "VulnerabilitySurface", "defensive_posture"),
        ("PieceActivity", "OperationalTempo", "resource_deployment"),
        ("TacticalThreats", "AttackVectors", "forcing_possibilities"),
        ("StrategicPlan", "DeploymentStrategy", "objective_pursuit"),
        ("GamePhase", "SystemMaturity", "lifecycle_stage"),
        ("OpeningTheory", "KnownPatterns", "knowledge_base"),
        ("EndgameTechnique", "CapabilityConversion", "advantage_conversion"),
        ("TimePressure", "DecisionLatency", "reasoning_constraints"),
    ]
}

// ============================================================================
// Graph population
// ============================================================================

/// Populate a neo4j-rs graph with chess knowledge + AI War bridge.
///
/// Creates:
/// - `:Position` nodes with FEN, eval_cp (from stonksfish), phase
/// - `:Opening` nodes with ECO code, name, PGN moves
/// - `:Opening -[:STARTS_AT]-> :Position` edges
/// - `:Position -[:PLAYS_TO {uci}]-> :Position` edges for common lines
/// - `:TacticalConcept` nodes for chess and AI War concepts
/// - `:TacticalConcept -[:MAPS_TO]-> :TacticalConcept` cross-domain bridge
pub async fn populate_chess_graph(graph: &Graph<MemoryBackend>) -> Result<PopulationStats> {
    let backend = graph.backend();
    let mut tx = backend.begin_tx(TxMode::ReadWrite).await?;
    let mut stats = PopulationStats::default();

    // --- 1. Create starting position node ---
    let startpos_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let startpos_id = create_position_node(backend, &mut tx, startpos_fen).await?;
    stats.positions_created += 1;

    // Track FEN → NodeId for edge creation
    let mut fen_to_node: HashMap<String, NodeId> = HashMap::new();
    fen_to_node.insert(startpos_fen.to_string(), startpos_id);

    // --- 2. Create opening nodes + position nodes ---
    let openings = seed_openings();
    for entry in &openings {
        // Create the opening's final position node
        let pos_id = if let Some(&id) = fen_to_node.get(entry.fen) {
            id
        } else {
            let id = create_position_node(backend, &mut tx, entry.fen).await?;
            fen_to_node.insert(entry.fen.to_string(), id);
            stats.positions_created += 1;
            id
        };

        // Create opening node
        let mut props = PropertyMap::new();
        props.insert("eco".into(), Value::String(entry.eco.into()));
        props.insert("name".into(), Value::String(entry.name.into()));
        props.insert("pgn".into(), Value::String(entry.pgn.into()));
        props.insert("fen".into(), Value::String(entry.fen.into()));
        let opening_id = backend.create_node(&mut tx, &["Opening"], props).await?;
        stats.openings_created += 1;

        // :Opening -[:STARTS_AT]-> :Position
        backend.create_relationship(&mut tx, opening_id, pos_id, "STARTS_AT", PropertyMap::new()).await?;
        stats.edges_created += 1;

        // :Position(start) -[:PLAYS_TO]-> :Position(opening)
        // Connect opening position to startpos via PLAYS_TO
        backend.create_relationship(&mut tx, startpos_id, pos_id, "PLAYS_TO", {
            let mut p = PropertyMap::new();
            p.insert("pgn".into(), Value::String(entry.pgn.into()));
            p.insert("eco".into(), Value::String(entry.eco.into()));
            p
        }).await?;
        stats.edges_created += 1;
    }

    // --- 3. Create inter-opening move edges (common transpositions) ---
    // e4 e5 positions chain
    let e4_fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    let e4_id = if let Some(&id) = fen_to_node.get(e4_fen) {
        id
    } else {
        let id = create_position_node(backend, &mut tx, e4_fen).await?;
        fen_to_node.insert(e4_fen.to_string(), id);
        stats.positions_created += 1;
        id
    };

    // startpos -[:PLAYS_TO {uci: "e2e4"}]-> e4
    backend.create_relationship(&mut tx, startpos_id, e4_id, "PLAYS_TO", {
        let mut p = PropertyMap::new();
        p.insert("uci".into(), Value::String("e2e4".into()));
        p.insert("san".into(), Value::String("e4".into()));
        p
    }).await?;
    stats.edges_created += 1;

    let d4_fen = "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3 0 1";
    let d4_id = if let Some(&id) = fen_to_node.get(d4_fen) {
        id
    } else {
        let id = create_position_node(backend, &mut tx, d4_fen).await?;
        fen_to_node.insert(d4_fen.to_string(), id);
        stats.positions_created += 1;
        id
    };

    backend.create_relationship(&mut tx, startpos_id, d4_id, "PLAYS_TO", {
        let mut p = PropertyMap::new();
        p.insert("uci".into(), Value::String("d2d4".into()));
        p.insert("san".into(), Value::String("d4".into()));
        p
    }).await?;
    stats.edges_created += 1;

    // --- 4. AI War tactical bridge ---
    let bridge = tactical_bridge();
    for (chess_concept, aiwar_concept, dimension) in &bridge {
        // Chess tactical concept
        let chess_id = backend.create_node(&mut tx, &["TacticalConcept", "Chess"], {
            let mut p = PropertyMap::new();
            p.insert("name".into(), Value::String(chess_concept.to_string()));
            p.insert("domain".into(), Value::String("chess".into()));
            p.insert("dimension".into(), Value::String(dimension.to_string()));
            p
        }).await?;

        // AI War tactical concept
        let aiwar_id = backend.create_node(&mut tx, &["TacticalConcept", "AIWar"], {
            let mut p = PropertyMap::new();
            p.insert("name".into(), Value::String(aiwar_concept.to_string()));
            p.insert("domain".into(), Value::String("aiwar".into()));
            p.insert("dimension".into(), Value::String(dimension.to_string()));
            p
        }).await?;

        // Cross-domain bridge edge
        backend.create_relationship(&mut tx, chess_id, aiwar_id, "MAPS_TO", {
            let mut p = PropertyMap::new();
            p.insert("dimension".into(), Value::String(dimension.to_string()));
            p.insert("bidirectional".into(), Value::Bool(true));
            p
        }).await?;

        stats.concepts_created += 2;
        stats.edges_created += 1;
    }

    backend.commit_tx(tx).await?;
    Ok(stats)
}

/// Create a `:Position` node with stonksfish evaluation.
async fn create_position_node<B: StorageBackend>(
    backend: &B,
    tx: &mut B::Tx,
    fen: &str,
) -> Result<NodeId> {
    let mut props = PropertyMap::new();
    props.insert("fen".into(), Value::String(fen.into()));

    // Evaluate with stonksfish
    if let Ok(board) = Board::from_str(fen) {
        let analysis = stonksfish::uci::analyze_position(&board, 5);
        props.insert("eval_cp".into(), Value::Int(analysis.eval_cp as i64));
        props.insert("phase".into(), Value::String(analysis.phase.clone()));
    }

    // Classify position type
    let piece_count = fen.chars().filter(|c| c.is_alphabetic() && *c != '/').count();
    let phase = if piece_count <= 10 {
        "endgame"
    } else if piece_count <= 24 {
        "middlegame"
    } else {
        "opening"
    };
    props.insert("game_phase".into(), Value::String(phase.into()));

    backend.create_node(tx, &["Position"], props).await
}

// ============================================================================
// Population statistics
// ============================================================================

/// Statistics from graph population.
#[derive(Debug, Default)]
pub struct PopulationStats {
    pub positions_created: usize,
    pub openings_created: usize,
    pub concepts_created: usize,
    pub edges_created: usize,
}

impl std::fmt::Display for PopulationStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PopulationStats {{ positions: {}, openings: {}, concepts: {}, edges: {} }}",
            self.positions_created, self.openings_created, self.concepts_created, self.edges_created,
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_populate_chess_graph() {
        let graph = Graph::open_memory().await.unwrap();
        let stats = populate_chess_graph(&graph).await.unwrap();

        assert!(stats.positions_created > 0, "Should create position nodes");
        assert!(stats.openings_created > 0, "Should create opening nodes");
        assert!(stats.concepts_created > 0, "Should create tactical concept nodes");
        assert!(stats.edges_created > 0, "Should create edges");

        // Verify we can query a position
        let backend = graph.backend();
        let tx = backend.begin_tx(TxMode::ReadOnly).await.unwrap();
        let positions = backend.nodes_by_label(&tx, "Position").await.unwrap();
        assert!(!positions.is_empty(), "Should have Position nodes");

        // Check starting position has eval
        let startpos = positions.iter()
            .find(|n| {
                n.properties.get("fen")
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR"))
                    .unwrap_or(false)
            });
        assert!(startpos.is_some(), "Should have starting position node");
        let sp = startpos.unwrap();
        assert!(sp.properties.get("eval_cp").is_some(), "Should have eval_cp from stonksfish");
        assert!(sp.properties.get("phase").is_some(), "Should have phase");
        backend.commit_tx(tx).await.unwrap();
    }

    #[tokio::test]
    async fn test_openings_have_starts_at_edges() {
        let graph = Graph::open_memory().await.unwrap();
        let _stats = populate_chess_graph(&graph).await.unwrap();

        let backend = graph.backend();
        let tx = backend.begin_tx(TxMode::ReadOnly).await.unwrap();

        let openings = backend.nodes_by_label(&tx, "Opening").await.unwrap();
        assert!(openings.len() >= 12, "Should have at least 12 openings, got {}", openings.len());

        // Each opening should have a STARTS_AT relationship
        for opening in &openings {
            let rels = backend.get_relationships(&tx, opening.id, crate::Direction::Outgoing, Some("STARTS_AT")).await.unwrap();
            assert!(!rels.is_empty(), "Opening {} should have STARTS_AT edge", opening.properties.get("name").unwrap());
        }

        backend.commit_tx(tx).await.unwrap();
    }

    #[tokio::test]
    async fn test_tactical_bridge_created() {
        let graph = Graph::open_memory().await.unwrap();
        let stats = populate_chess_graph(&graph).await.unwrap();

        // 10 chess + 10 aiwar concepts
        assert_eq!(stats.concepts_created, 20);

        let backend = graph.backend();
        let tx = backend.begin_tx(TxMode::ReadOnly).await.unwrap();

        let chess_concepts = backend.nodes_by_label(&tx, "Chess").await.unwrap();
        let aiwar_concepts = backend.nodes_by_label(&tx, "AIWar").await.unwrap();
        assert_eq!(chess_concepts.len(), 10);
        assert_eq!(aiwar_concepts.len(), 10);

        backend.commit_tx(tx).await.unwrap();
    }

    #[test]
    fn test_seed_openings_all_valid_fen() {
        for entry in seed_openings() {
            assert!(
                Board::from_str(entry.fen).is_ok(),
                "Invalid FEN for {}: {}", entry.name, entry.fen,
            );
        }
    }
}
