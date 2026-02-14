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
//! These procedures are registered as named handlers in a `ChessProcedureHandler`.
//! The handler maps procedure names (e.g. `"chess.evaluate"`) to functions that
//! accept `Vec<Value>` arguments and return `ProcedureResult`.
//!
//! Currently returns mock data. The real evaluation pipeline will wire through:
//! - **stonksfish** for static evaluation (eval_cp, phase detection)
//! - **ladybug-rs** for Hamming-accelerated fingerprint similarity search
//!
//! ## Integration
//!
//! A `StorageBackend` implementation can delegate `call_procedure()` calls
//! whose name starts with `"chess."` to `ChessProcedureHandler::call()`.

use std::collections::HashMap;

use crate::model::Value;
use crate::storage::ProcedureResult;
use crate::{Error, Result};

// ============================================================================
// Procedure handler function type
// ============================================================================

/// A chess procedure handler: takes arguments, returns columnar results.
///
/// Each handler validates its own arguments and produces rows with the
/// columns declared in the procedure's YIELD clause.
pub type ProcedureFn = fn(args: Vec<Value>) -> Result<ProcedureResult>;

// ============================================================================
// ChessProcedureHandler
// ============================================================================

/// Registry of chess-domain Cypher procedures.
///
/// Holds a `HashMap<String, ProcedureFn>` mapping fully-qualified procedure
/// names (e.g. `"chess.evaluate"`) to their handler functions.
///
/// # Example
///
/// ```rust,no_run
/// use neo4j_rs::chess::ChessProcedureHandler;
/// use neo4j_rs::model::Value;
///
/// let handler = ChessProcedureHandler::new();
/// let result = handler.call("chess.evaluate", vec![
///     Value::String("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1".into()),
/// ]).unwrap();
///
/// assert_eq!(result.columns, vec!["eval_cp", "phase"]);
/// assert_eq!(result.rows.len(), 1);
/// ```
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
    ///
    /// Returns `Error::ExecutionError` if the procedure name is not registered.
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
///
/// Returns a `HashMap` mapping each procedure's fully-qualified name to its
/// handler function. Add new procedures here.
pub fn register_chess_procedures() -> HashMap<String, ProcedureFn> {
    let mut map: HashMap<String, ProcedureFn> = HashMap::new();
    map.insert("chess.evaluate".into(), proc_evaluate);
    map.insert("chess.similar".into(), proc_similar);
    map.insert("chess.opening_lookup".into(), proc_opening_lookup);
    map
}

// ============================================================================
// chess.evaluate — static position evaluation
// ============================================================================

/// `CALL chess.evaluate($fen) YIELD eval_cp, phase`
///
/// Evaluates a chess position given as a FEN string.
///
/// ## Arguments
///
/// | Index | Name | Type   | Description                    |
/// |-------|------|--------|--------------------------------|
/// | 0     | fen  | STRING | FEN string of the position     |
///
/// ## Yield columns
///
/// | Column  | Type    | Description                                    |
/// |---------|---------|------------------------------------------------|
/// | eval_cp | INT     | Evaluation in centipawns (positive = white)    |
/// | phase   | STRING  | Game phase: "opening", "middlegame", "endgame" |
///
/// ## Mock behavior
///
/// Returns a deterministic mock evaluation derived from the FEN string's
/// byte hash. Will be replaced by stonksfish integration.
fn proc_evaluate(args: Vec<Value>) -> Result<ProcedureResult> {
    // Validate: exactly 1 argument, must be a string (FEN)
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

    // Validate FEN has at least the board part (contains '/')
    if !fen.contains('/') {
        return Err(Error::ExecutionError(format!(
            "chess.evaluate(): invalid FEN string: '{}'", fen,
        )));
    }

    // Mock evaluation: deterministic hash of FEN → centipawn value
    let hash = fen_hash(fen);
    let eval_cp = ((hash % 601) as i64) - 300; // range: -300..+300 cp

    // Mock phase detection based on piece count in the FEN board part
    let board_part = fen.split_whitespace().next().unwrap_or(fen);
    let piece_count = board_part.chars().filter(|c| c.is_alphabetic()).count();
    let phase = if piece_count >= 28 {
        "opening"
    } else if piece_count >= 14 {
        "middlegame"
    } else {
        "endgame"
    };

    let mut row = HashMap::new();
    row.insert("eval_cp".into(), Value::Int(eval_cp));
    row.insert("phase".into(), Value::String(phase.into()));

    Ok(ProcedureResult {
        columns: vec!["eval_cp".into(), "phase".into()],
        rows: vec![row],
    })
}

// ============================================================================
// chess.similar — fingerprint similarity search
// ============================================================================

/// `CALL chess.similar($fen, $k) YIELD fen, similarity`
///
/// Find the k most similar positions to the given FEN by Hamming fingerprint
/// distance.
///
/// ## Arguments
///
/// | Index | Name | Type   | Description                            |
/// |-------|------|--------|----------------------------------------|
/// | 0     | fen  | STRING | FEN string of the query position       |
/// | 1     | k    | INT    | Number of similar positions to return  |
///
/// ## Yield columns
///
/// | Column     | Type   | Description                                  |
/// |------------|--------|----------------------------------------------|
/// | fen        | STRING | FEN of a similar position                    |
/// | similarity | FLOAT  | Similarity score in [0.0, 1.0] (1.0 = exact) |
///
/// ## Mock behavior
///
/// Returns k mock positions with decreasing similarity scores. The real
/// implementation will use ladybug-rs Hamming-accelerated DN-Tree search.
fn proc_similar(args: Vec<Value>) -> Result<ProcedureResult> {
    // Validate: exactly 2 arguments
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

    // Cap k to a reasonable maximum for mock data
    let k = k.min(20);

    if !fen.contains('/') {
        return Err(Error::ExecutionError(format!(
            "chess.similar(): invalid FEN string: '{}'", fen,
        )));
    }

    // Generate k mock similar positions with decreasing similarity
    let mock_fens = [
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
        "rnbqkbnr/ppppp1pp/5p2/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        "rnbqkbnr/pppppppp/8/8/8/1P6/P1PPPPPP/RNBQKBNR b KQkq - 0 1",
        "rnbqkbnr/pppp1ppp/8/4p3/2P5/8/PP1PPPPP/RNBQKBNR w KQkq e6 0 2",
        "rnbqkbnr/pppppp1p/6p1/8/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2",
        "rnbqkbnr/pppppppp/8/8/6P1/8/PPPPPP1P/RNBQKBNR b KQkq g3 0 1",
    ];

    let rows: Vec<HashMap<String, Value>> = (0..k)
        .map(|i| {
            let similarity = 1.0 - (i as f64 * 0.04); // 1.0, 0.96, 0.92, ...
            let result_fen = mock_fens[i % mock_fens.len()];
            let mut row = HashMap::new();
            row.insert("fen".into(), Value::String(result_fen.into()));
            row.insert("similarity".into(), Value::Float(similarity));
            row
        })
        .collect();

    Ok(ProcedureResult {
        columns: vec!["fen".into(), "similarity".into()],
        rows,
    })
}

// ============================================================================
// chess.opening_lookup — ECO opening classification
// ============================================================================

/// `CALL chess.opening_lookup($fen) YIELD name, eco, moves`
///
/// Look up the opening name and ECO code for a position.
///
/// ## Arguments
///
/// | Index | Name | Type   | Description                    |
/// |-------|------|--------|--------------------------------|
/// | 0     | fen  | STRING | FEN string of the position     |
///
/// ## Yield columns
///
/// | Column | Type   | Description                           |
/// |--------|--------|---------------------------------------|
/// | name   | STRING | Opening name (e.g. "Sicilian Defense") |
/// | eco    | STRING | ECO code (e.g. "B20")                 |
/// | moves  | STRING | Move sequence leading to this position|
///
/// ## Mock behavior
///
/// Returns a mock opening classification based on hashing the FEN.
/// The real implementation will query an ECO database or opening book.
fn proc_opening_lookup(args: Vec<Value>) -> Result<ProcedureResult> {
    // Validate: exactly 1 argument
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

    // Mock opening database: select based on FEN hash
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

/// Simple deterministic hash of a FEN string for mock data generation.
///
/// This is NOT a cryptographic hash. It produces a stable usize from the
/// FEN bytes so that the same FEN always yields the same mock results.
fn fen_hash(fen: &str) -> usize {
    // djb2 hash
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

    // ========================================================================
    // chess.evaluate tests
    // ========================================================================

    #[test]
    fn test_evaluate_returns_eval_and_phase() {
        let result = proc_evaluate(vec![Value::String(STARTING_FEN.into())]).unwrap();
        assert_eq!(result.columns, vec!["eval_cp", "phase"]);
        assert_eq!(result.rows.len(), 1);

        let row = &result.rows[0];
        assert!(row.get("eval_cp").unwrap().as_int().is_some());

        let phase = row.get("phase").unwrap().as_str().unwrap();
        assert!(
            ["opening", "middlegame", "endgame"].contains(&phase),
            "unexpected phase: {phase}"
        );
    }

    #[test]
    fn test_evaluate_starting_position_is_opening() {
        let result = proc_evaluate(vec![Value::String(STARTING_FEN.into())]).unwrap();
        let phase = result.rows[0].get("phase").unwrap().as_str().unwrap();
        // Starting position has 32 pieces: should be "opening"
        assert_eq!(phase, "opening");
    }

    #[test]
    fn test_evaluate_endgame_detected() {
        let result = proc_evaluate(vec![Value::String(ENDGAME_FEN.into())]).unwrap();
        let phase = result.rows[0].get("phase").unwrap().as_str().unwrap();
        // K+R vs K = 3 pieces: should be "endgame"
        assert_eq!(phase, "endgame");
    }

    #[test]
    fn test_evaluate_deterministic() {
        let r1 = proc_evaluate(vec![Value::String(E4_FEN.into())]).unwrap();
        let r2 = proc_evaluate(vec![Value::String(E4_FEN.into())]).unwrap();
        assert_eq!(
            r1.rows[0].get("eval_cp"),
            r2.rows[0].get("eval_cp"),
            "Same FEN must produce same evaluation"
        );
    }

    #[test]
    fn test_evaluate_wrong_arg_count() {
        let result = proc_evaluate(vec![]);
        assert!(result.is_err());

        let result = proc_evaluate(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(5),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_evaluate_wrong_arg_type() {
        let result = proc_evaluate(vec![Value::Int(42)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_evaluate_invalid_fen() {
        let result = proc_evaluate(vec![Value::String("not a fen".into())]);
        assert!(result.is_err());
    }

    // ========================================================================
    // chess.similar tests
    // ========================================================================

    #[test]
    fn test_similar_returns_k_results() {
        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(5),
        ]).unwrap();

        assert_eq!(result.columns, vec!["fen", "similarity"]);
        assert_eq!(result.rows.len(), 5);
    }

    #[test]
    fn test_similar_decreasing_similarity() {
        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(10),
        ]).unwrap();

        let scores: Vec<f64> = result.rows.iter()
            .map(|r| r.get("similarity").unwrap().as_float().unwrap())
            .collect();

        for window in scores.windows(2) {
            assert!(
                window[0] >= window[1],
                "Similarity scores should be non-increasing: {:?}",
                scores,
            );
        }
    }

    #[test]
    fn test_similar_all_results_have_fen() {
        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(3),
        ]).unwrap();

        for row in &result.rows {
            let fen = row.get("fen").unwrap().as_str().unwrap();
            assert!(fen.contains('/'), "Result FEN should contain '/': {fen}");
        }
    }

    #[test]
    fn test_similar_capped_at_20() {
        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(100),
        ]).unwrap();

        assert_eq!(result.rows.len(), 20, "k should be capped at 20 for mock data");
    }

    #[test]
    fn test_similar_wrong_arg_count() {
        assert!(proc_similar(vec![]).is_err());
        assert!(proc_similar(vec![Value::String(STARTING_FEN.into())]).is_err());
    }

    #[test]
    fn test_similar_k_must_be_positive() {
        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(0),
        ]);
        assert!(result.is_err());

        let result = proc_similar(vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(-5),
        ]);
        assert!(result.is_err());
    }

    // ========================================================================
    // chess.opening_lookup tests
    // ========================================================================

    #[test]
    fn test_opening_lookup_returns_columns() {
        let result = proc_opening_lookup(vec![
            Value::String(E4_FEN.into()),
        ]).unwrap();

        assert_eq!(result.columns, vec!["name", "eco", "moves"]);
        assert_eq!(result.rows.len(), 1);

        let row = &result.rows[0];
        assert!(row.get("name").unwrap().as_str().is_some());
        assert!(row.get("eco").unwrap().as_str().is_some());
        assert!(row.get("moves").unwrap().as_str().is_some());
    }

    #[test]
    fn test_opening_lookup_eco_format() {
        let result = proc_opening_lookup(vec![
            Value::String(STARTING_FEN.into()),
        ]).unwrap();

        let eco = result.rows[0].get("eco").unwrap().as_str().unwrap();
        // ECO codes are a letter A-E followed by 2 digits
        assert!(eco.len() == 3, "ECO code should be 3 characters: {eco}");
        assert!(eco.as_bytes()[0].is_ascii_alphabetic());
        assert!(eco.as_bytes()[1].is_ascii_digit());
        assert!(eco.as_bytes()[2].is_ascii_digit());
    }

    #[test]
    fn test_opening_lookup_deterministic() {
        let r1 = proc_opening_lookup(vec![Value::String(E4_FEN.into())]).unwrap();
        let r2 = proc_opening_lookup(vec![Value::String(E4_FEN.into())]).unwrap();
        assert_eq!(r1.rows[0].get("name"), r2.rows[0].get("name"));
        assert_eq!(r1.rows[0].get("eco"), r2.rows[0].get("eco"));
    }

    #[test]
    fn test_opening_lookup_wrong_args() {
        assert!(proc_opening_lookup(vec![]).is_err());
        assert!(proc_opening_lookup(vec![Value::Int(1)]).is_err());
        assert!(proc_opening_lookup(vec![Value::String("bad".into())]).is_err());
    }

    // ========================================================================
    // ChessProcedureHandler tests
    // ========================================================================

    #[test]
    fn test_handler_has_all_procedures() {
        let handler = ChessProcedureHandler::new();
        assert!(handler.has_procedure("chess.evaluate"));
        assert!(handler.has_procedure("chess.similar"));
        assert!(handler.has_procedure("chess.opening_lookup"));
        assert!(!handler.has_procedure("chess.nonexistent"));
    }

    #[test]
    fn test_handler_call_dispatches() {
        let handler = ChessProcedureHandler::new();

        let result = handler.call("chess.evaluate", vec![
            Value::String(STARTING_FEN.into()),
        ]).unwrap();
        assert_eq!(result.columns, vec!["eval_cp", "phase"]);

        let result = handler.call("chess.similar", vec![
            Value::String(STARTING_FEN.into()),
            Value::Int(3),
        ]).unwrap();
        assert_eq!(result.columns, vec!["fen", "similarity"]);
        assert_eq!(result.rows.len(), 3);
    }

    #[test]
    fn test_handler_unknown_procedure_error() {
        let handler = ChessProcedureHandler::new();
        let result = handler.call("chess.nonexistent", vec![]);
        assert!(result.is_err());
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

    #[test]
    fn test_register_chess_procedures_returns_all() {
        let map = register_chess_procedures();
        assert_eq!(map.len(), 3);
        assert!(map.contains_key("chess.evaluate"));
        assert!(map.contains_key("chess.similar"));
        assert!(map.contains_key("chess.opening_lookup"));
    }

    // ========================================================================
    // fen_hash tests
    // ========================================================================

    #[test]
    fn test_fen_hash_deterministic() {
        assert_eq!(fen_hash(STARTING_FEN), fen_hash(STARTING_FEN));
    }

    #[test]
    fn test_fen_hash_different_inputs() {
        assert_ne!(fen_hash(STARTING_FEN), fen_hash(E4_FEN));
    }
}
