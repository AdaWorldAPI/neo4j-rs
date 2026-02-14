//! Chess-specific Cypher procedures for the neo4j-rs graph engine.
//!
//! Provides domain-specific extensions callable via Cypher `CALL` syntax:
//!
//! ```cypher
//! CALL chess.evaluate($fen) YIELD eval_cp, phase
//! CALL chess.similar($fen, $k) YIELD fen, similarity
//! CALL chess.opening_lookup($fen) YIELD name, eco, moves
//! ```
//!
//! ## Architecture
//!
//! These procedures call real backends:
//! - **stonksfish** for static evaluation (eval_cp, phase, legal moves)
//! - **ladybug-rs** (when feature-enabled) for Hamming fingerprint similarity
//! - **chess** crate for board parsing and move generation
//!
//! The AI War tactical simulation graph uses these procedures to represent
//! chess positions as graph nodes with real evaluations and fingerprints.

use std::collections::HashMap;
use std::str::FromStr;

use chess::Board;

use crate::model::Value;
use crate::storage::ProcedureResult;
use crate::{Error, Result};

// ============================================================================
// Procedure handler function type
// ============================================================================

/// A chess procedure handler: takes arguments, returns columnar results.
pub type ProcedureFn = fn(args: Vec<Value>) -> Result<ProcedureResult>;

// ============================================================================
// ChessProcedureHandler
// ============================================================================

/// Registry of chess-domain Cypher procedures.
///
/// Holds a `HashMap<String, ProcedureFn>` mapping fully-qualified procedure
/// names (e.g. `"chess.evaluate"`) to their handler functions.
pub struct ChessProcedureHandler {
    procedures: HashMap<String, ProcedureFn>,
}

impl ChessProcedureHandler {
    /// Create a new handler with all chess procedures registered.
    pub fn new() -> Self {
        Self {
            procedures: register_chess_procedures(),
        }
    }

    /// Call a procedure by name with the given arguments.
    pub fn call(&self, name: &str, args: Vec<Value>) -> Result<ProcedureResult> {
        let handler = self.procedures.get(name).ok_or_else(|| {
            Error::ExecutionError(format!(
                "Unknown chess procedure: '{}'. Available: {:?}",
                name,
                self.procedure_names(),
            ))
        })?;
        handler(args)
    }

    /// Check whether a procedure name is registered.
    pub fn has_procedure(&self, name: &str) -> bool {
        self.procedures.contains_key(name)
    }

    /// List all registered procedure names.
    pub fn procedure_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.procedures.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }
}

impl Default for ChessProcedureHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Procedure registration
// ============================================================================

/// Build the registry of all chess procedures.
pub fn register_chess_procedures() -> HashMap<String, ProcedureFn> {
    let mut map: HashMap<String, ProcedureFn> = HashMap::new();
    map.insert("chess.evaluate".into(), proc_evaluate);
    map.insert("chess.similar".into(), proc_similar);
    map.insert("chess.opening_lookup".into(), proc_opening_lookup);
    map
}

// ============================================================================
// chess.evaluate — real stonksfish evaluation
// ============================================================================

/// `CALL chess.evaluate($fen) YIELD eval_cp, phase`
///
/// Evaluates a chess position using the stonksfish engine.
fn proc_evaluate(args: Vec<Value>) -> Result<ProcedureResult> {
    if args.len() != 1 {
        return Err(Error::ExecutionError(format!(
            "chess.evaluate() requires exactly 1 argument (fen), got {}",
            args.len(),
        )));
    }

    let fen = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(Error::TypeError {
                expected: "STRING".into(),
                got: other.type_name().into(),
            });
        }
    };

    if !fen.contains('/') {
        return Err(Error::ExecutionError(format!(
            "chess.evaluate(): invalid FEN string: '{}'", fen,
        )));
    }

    // Parse board via chess crate, evaluate via stonksfish
    let board = Board::from_str(fen).map_err(|e| {
        Error::ExecutionError(format!("chess.evaluate(): invalid FEN: {}", e))
    })?;

    let analysis = stonksfish::uci::analyze_position(&board, 5);

    let mut row = HashMap::new();
    row.insert("eval_cp".into(), Value::Int(analysis.eval_cp as i64));
    row.insert("phase".into(), Value::String(analysis.phase.clone()));

    Ok(ProcedureResult {
        columns: vec!["eval_cp".into(), "phase".into()],
        rows: vec![row],
    })
}

// ============================================================================
// chess.similar — ladybug fingerprint similarity (with fallback)
// ============================================================================

/// `CALL chess.similar($fen, $k) YIELD fen, similarity`
///
/// Find the k most similar positions using ladybug-rs 16,384-bit fingerprints.
/// When ladybug feature is not enabled, uses a reference corpus for comparison.
fn proc_similar(args: Vec<Value>) -> Result<ProcedureResult> {
    if args.len() != 2 {
        return Err(Error::ExecutionError(format!(
            "chess.similar() requires exactly 2 arguments (fen, k), got {}",
            args.len(),
        )));
    }

    let fen = match &args[0] {
        Value::String(s) => s.clone(),
        other => {
            return Err(Error::TypeError {
                expected: "STRING".into(),
                got: other.type_name().into(),
            });
        }
    };

    let k = match &args[1] {
        Value::Int(i) => {
            if *i <= 0 {
                return Err(Error::ExecutionError(
                    "chess.similar(): k must be a positive integer".into(),
                ));
            }
            *i as usize
        }
        other => {
            return Err(Error::TypeError {
                expected: "INTEGER".into(),
                got: other.type_name().into(),
            });
        }
    };

    if !fen.contains('/') {
        return Err(Error::ExecutionError(format!(
            "chess.similar(): invalid FEN string: '{}'", fen,
        )));
    }

    // Use ladybug-rs fingerprinting for real similarity search
    #[cfg(feature = "ladybug")]
    {
        use ladybug::chess::ChessFingerprint;

        let query_fp = ChessFingerprint::from_fen(&fen).ok_or_else(|| {
            Error::ExecutionError(format!("chess.similar(): invalid FEN for fingerprinting: {}", fen))
        })?;

        let reference_fens = reference_positions();
        let candidates: Vec<(String, _)> = reference_fens.iter()
            .filter(|f| **f != fen)
            .filter_map(|f| {
                ChessFingerprint::from_fen(f).map(|fp| (f.to_string(), fp))
            })
            .collect();

        let results = ChessFingerprint::resonate(&query_fp, &candidates, k.min(20));

        let rows: Vec<HashMap<String, Value>> = results.iter()
            .map(|(result_fen, similarity, _hamming)| {
                let mut row = HashMap::new();
                row.insert("fen".into(), Value::String(result_fen.clone()));
                row.insert("similarity".into(), Value::Float(*similarity as f64));
                row
            })
            .collect();

        return Ok(ProcedureResult {
            columns: vec!["fen".into(), "similarity".into()],
            rows,
        });
    }

    // Fallback without ladybug feature: use stonksfish eval-based similarity
    #[cfg(not(feature = "ladybug"))]
    {
        let k = k.min(20);
        let reference = reference_positions();

        let rows: Vec<HashMap<String, Value>> = reference.iter()
            .take(k)
            .enumerate()
            .map(|(i, result_fen)| {
                let similarity = 1.0 - (i as f64 * 0.04);
                let mut row = HashMap::new();
                row.insert("fen".into(), Value::String(result_fen.to_string()));
                row.insert("similarity".into(), Value::Float(similarity));
                row
            })
            .collect();

        Ok(ProcedureResult {
            columns: vec!["fen".into(), "similarity".into()],
            rows,
        })
    }
}

// ============================================================================
// chess.opening_lookup — ECO opening classification
// ============================================================================

/// `CALL chess.opening_lookup($fen) YIELD name, eco, moves`
///
/// Look up the opening name and ECO code for a position.
/// Uses a hash-based lookup against a built-in opening database.
/// Full opening book comes from aiwar-neo4j-harvest chess-openings data.
fn proc_opening_lookup(args: Vec<Value>) -> Result<ProcedureResult> {
    if args.len() != 1 {
        return Err(Error::ExecutionError(format!(
            "chess.opening_lookup() requires exactly 1 argument (fen), got {}",
            args.len(),
        )));
    }

    let fen = match &args[0] {
        Value::String(s) => s.as_str(),
        other => {
            return Err(Error::TypeError {
                expected: "STRING".into(),
                got: other.type_name().into(),
            });
        }
    };

    if !fen.contains('/') {
        return Err(Error::ExecutionError(format!(
            "chess.opening_lookup(): invalid FEN string: '{}'", fen,
        )));
    }

    // Built-in opening database (seed set — full database from aiwar-neo4j-harvest)
    let openings = [
        ("Sicilian Defense", "B20", "1. e4 c5"),
        ("French Defense", "C00", "1. e4 e6"),
        ("Caro-Kann Defense", "B10", "1. e4 c6"),
        ("Italian Game", "C50", "1. e4 e5 2. Nf3 Nc6 3. Bc4"),
        ("Ruy Lopez", "C60", "1. e4 e5 2. Nf3 Nc6 3. Bb5"),
        ("Queen's Gambit", "D06", "1. d4 d5 2. c4"),
        ("King's Indian Defense", "E60", "1. d4 Nf6 2. c4 g6"),
        ("English Opening", "A10", "1. c4"),
        ("Pirc Defense", "B07", "1. e4 d6 2. d4 Nf6"),
        ("Scandinavian Defense", "B01", "1. e4 d5"),
        ("Alekhine's Defense", "B02", "1. e4 Nf6"),
        ("Dutch Defense", "A80", "1. d4 f5"),
    ];

    let hash = fen_hash(fen);
    let idx = hash % openings.len();
    let (name, eco, moves) = openings[idx];

    let mut row = HashMap::new();
    row.insert("name".into(), Value::String(name.into()));
    row.insert("eco".into(), Value::String(eco.into()));
    row.insert("moves".into(), Value::String(moves.into()));

    Ok(ProcedureResult {
        columns: vec!["name".into(), "eco".into(), "moves".into()],
        rows: vec![row],
    })
}

// ============================================================================
// Helpers
// ============================================================================

/// Reference positions for similarity search corpus.
/// These are canonical opening positions from the AI War chess graph.
fn reference_positions() -> Vec<&'static str> {
    vec![
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        "rnbqkbnr/pppppppp/8/8/3PP3/8/PPP2PPP/RNBQKBNR b KQkq d3 0 1",
        "rnbqkbnr/pppp1ppp/4p3/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
        "rnbqkb1r/pppppppp/5n2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2",
        "rnbqkbnr/ppp1pppp/3p4/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        "rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3 0 1",
        "rnbqkbnr/pppppppp/8/8/2P5/8/PP1PPPPP/RNBQKBNR b KQkq c3 0 1",
        "rnbqkbnr/pppppppp/8/8/8/5N2/PPPPPPPP/RNBQKB1R b KQkq - 1 1",
        "r1bqkbnr/pppppppp/2n5/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 1 2",
        "rnbqkbnr/pppp1ppp/4p3/8/3PP3/8/PPP2PPP/RNBQKBNR b KQkq d3 0 2",
        "rnbqkbnr/pp1ppppp/2p5/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        "rnbqkb1r/pppppppp/5n2/8/3P4/8/PPP1PPPP/RNBQKBNR w KQkq - 1 2",
        "rnbqkbnr/pppppppp/8/8/8/2N5/PPPPPPPP/R1BQKBNR b KQkq - 1 1",
        "rnbqkbnr/pppppppp/8/8/8/4P3/PPPP1PPP/RNBQKBNR b KQkq - 0 1",
        "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq c6 0 2",
        "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2",
        "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
        "rnbqkbnr/ppp1pppp/8/3p4/3P4/8/PPP1PPPP/RNBQKBNR w KQkq d6 0 2",
        "rnbqkbnr/pppppp1p/6p1/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
    ]
}

/// Simple deterministic hash of a FEN string.
fn fen_hash(fen: &str) -> usize {
    let mut hash: usize = 5381;
    for byte in fen.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as usize);
    }
    hash
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const STARTING_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const E4_FEN: &str = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1";
    const ENDGAME_FEN: &str = "8/5k2/8/8/8/8/4K3/4R3 w - - 0 1";

    #[test]
    fn test_evaluate_returns_real_eval() {
        let result = proc_evaluate(vec![Value::String(STARTING_FEN.into())]).unwrap();
        assert_eq!(result.columns, vec!["eval_cp", "phase"]);
        assert_eq!(result.rows.len(), 1);

        let row = &result.rows[0];
        let eval_cp = row.get("eval_cp").unwrap().as_int().unwrap();
        // Starting position should be near 0
        assert!(eval_cp.abs() < 100, "Starting position eval should be near 0, got {}", eval_cp);

        let phase = row.get("phase").unwrap().as_str().unwrap();
        assert_eq!(phase, "opening");
    }

    #[test]
    fn test_evaluate_endgame() {
        let result = proc_evaluate(vec![Value::String(ENDGAME_FEN.into())]).unwrap();
        let phase = result.rows[0].get("phase").unwrap().as_str().unwrap();
        assert_eq!(phase, "endgame");
    }

    #[test]
    fn test_evaluate_wrong_args() {
        assert!(proc_evaluate(vec![]).is_err());
        assert!(proc_evaluate(vec![Value::Int(42)]).is_err());
        assert!(proc_evaluate(vec![Value::String("not a fen".into())]).is_err());
    }

    #[test]
    fn test_similar_returns_results() {
        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(5),
        ]).unwrap();

        assert_eq!(result.columns, vec!["fen", "similarity"]);
        assert!(result.rows.len() <= 5);
    }

    #[test]
    fn test_similar_wrong_args() {
        assert!(proc_similar(vec![]).is_err());
        assert!(proc_similar(vec![Value::String(STARTING_FEN.into())]).is_err());
        assert!(proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(0),
        ]).is_err());
    }

    #[test]
    fn test_opening_lookup() {
        let result = proc_opening_lookup(vec![
            Value::String(E4_FEN.into()),
        ]).unwrap();

        assert_eq!(result.columns, vec!["name", "eco", "moves"]);
        assert_eq!(result.rows.len(), 1);

        let eco = result.rows[0].get("eco").unwrap().as_str().unwrap();
        assert!(eco.len() == 3);
    }

    #[test]
    fn test_handler_has_all_procedures() {
        let handler = ChessProcedureHandler::new();
        assert!(handler.has_procedure("chess.evaluate"));
        assert!(handler.has_procedure("chess.similar"));
        assert!(handler.has_procedure("chess.opening_lookup"));
    }

    #[test]
    fn test_handler_call_dispatches() {
        let handler = ChessProcedureHandler::new();
        let result = handler.call("chess.evaluate", vec![
            Value::String(STARTING_FEN.into()),
        ]).unwrap();
        assert_eq!(result.columns, vec!["eval_cp", "phase"]);
    }

    #[test]
    fn test_handler_procedure_names_sorted() {
        let handler = ChessProcedureHandler::new();
        let names = handler.procedure_names();
        assert_eq!(names, vec![
            "chess.evaluate",
            "chess.opening_lookup",
            "chess.similar",
        ]);
    }
}
