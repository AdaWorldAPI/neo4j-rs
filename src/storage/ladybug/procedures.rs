//! ladybug.* procedure handlers.
//!
//! Registered procedures callable via `CALL ladybug.search(...)` etc.
//! These translate Neo4j CALL semantics into ladybug cognitive operations.

use std::collections::HashMap;

use crate::model::Value;
use crate::storage::ProcedureResult;
use crate::Result;
use crate::Error;

use super::fingerprint::{ContainerDto, PropertyFingerprinter, siphash_string};

// ============================================================================
// Procedure registry
// ============================================================================

/// All registered ladybug.* procedures and their metadata.
pub static PROCEDURE_NAMES: &[&str] = &[
    "ladybug.search",       // Resonance search by fingerprint
    "ladybug.bind",         // XOR bind two fingerprints
    "ladybug.unbind",       // XOR unbind (same as bind, XOR is self-inverse)
    "ladybug.similarity",   // Hamming similarity between two fingerprints
    "ladybug.fingerprint",  // Fingerprint a string or property map
    "ladybug.truth",        // Create or query NARS truth value
    "ladybug.revision",     // NARS truth revision (combine evidence)
    "ladybug.deduction",    // NARS deduction: A→B, B→C ⊢ A→C
    "ladybug.crystallize",  // Mark a belief as frozen (high confidence)
    "ladybug.spine",        // XOR-fold query over subtree
    "ladybug.spo.trace",    // Compute SPO holographic trace
    "ladybug.spo.recover",  // Recover missing SPO component via XOR
    "ladybug.abduction",    // NARS abduction: A→B, B ⊢ A (weak)
    "ladybug.induction",    // NARS induction: A, A→B ⊢ A→B (generalise)
];

/// Dispatch a procedure call to the appropriate handler.
pub fn dispatch(
    name: &str,
    args: &[Value],
    nodes: &HashMap<crate::model::NodeId, crate::model::Node>,
) -> Result<ProcedureResult> {
    match name {
        "ladybug.search" => proc_search(args, nodes),
        "ladybug.bind" => proc_bind(args),
        "ladybug.unbind" => proc_bind(args), // XOR is self-inverse
        "ladybug.similarity" => proc_similarity(args),
        "ladybug.fingerprint" => proc_fingerprint(args),
        "ladybug.truth" => proc_truth(args),
        "ladybug.revision" => proc_revision(args),
        "ladybug.deduction" => proc_deduction(args),
        "ladybug.abduction" => proc_abduction(args),
        "ladybug.induction" => proc_induction(args),
        "ladybug.crystallize" => proc_crystallize(args),
        "ladybug.spine" => proc_spine(args, nodes),
        "ladybug.spo.trace" => super::spo::proc_spo_trace(args),
        "ladybug.spo.recover" => super::spo::proc_spo_recover(args),
        _ => Err(Error::ExecutionError(format!("Unknown procedure: {name}"))),
    }
}

// ============================================================================
// ladybug.search(query_string, k) → (nodeId, score)
// ============================================================================

fn proc_search(
    args: &[Value],
    nodes: &HashMap<crate::model::NodeId, crate::model::Node>,
) -> Result<ProcedureResult> {
    let query_str = args.first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.search requires a string argument".into()))?;
    let k = args.get(1)
        .and_then(|v| v.as_int())
        .unwrap_or(10) as usize;

    let query_fp = ContainerDto::random(siphash_string(query_str));
    let fp = PropertyFingerprinter::cam();

    let mut scored: Vec<(crate::model::NodeId, f32)> = nodes.iter()
        .map(|(&id, node)| {
            let node_fp = fp.fingerprint(&node.properties);
            let sim = query_fp.similarity(&node_fp);
            (id, sim)
        })
        .collect();

    // Sort by similarity descending
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);

    let mut result = ProcedureResult {
        columns: vec!["nodeId".to_string(), "score".to_string()],
        rows: Vec::with_capacity(scored.len()),
    };

    for (id, score) in scored {
        let mut row = HashMap::new();
        row.insert("nodeId".to_string(), Value::Int(id.0 as i64));
        row.insert("score".to_string(), Value::Float(score as f64));
        result.rows.push(row);
    }

    Ok(result)
}

// ============================================================================
// ladybug.bind(a, b) → fingerprint bytes
// ============================================================================

fn proc_bind(args: &[Value]) -> Result<ProcedureResult> {
    let a_str = args.first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.bind requires two string arguments".into()))?;
    let b_str = args.get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.bind requires two string arguments".into()))?;

    let a = ContainerDto::random(siphash_string(a_str));
    let b = ContainerDto::random(siphash_string(b_str));
    let bound = a.xor(&b);

    let mut row = HashMap::new();
    row.insert("fingerprint".to_string(), Value::Bytes(bound.as_bytes().to_vec()));
    row.insert("popcount".to_string(), Value::Int(bound.popcount() as i64));

    Ok(ProcedureResult {
        columns: vec!["fingerprint".to_string(), "popcount".to_string()],
        rows: vec![row],
    })
}

// ============================================================================
// ladybug.similarity(a, b) → score
// ============================================================================

fn proc_similarity(args: &[Value]) -> Result<ProcedureResult> {
    let a_str = args.first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.similarity requires two string arguments".into()))?;
    let b_str = args.get(1)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.similarity requires two string arguments".into()))?;

    let a = ContainerDto::random(siphash_string(a_str));
    let b = ContainerDto::random(siphash_string(b_str));
    let sim = a.similarity(&b);
    let hamming = a.hamming(&b);

    let mut row = HashMap::new();
    row.insert("similarity".to_string(), Value::Float(sim as f64));
    row.insert("hamming".to_string(), Value::Int(hamming as i64));

    Ok(ProcedureResult {
        columns: vec!["similarity".to_string(), "hamming".to_string()],
        rows: vec![row],
    })
}

// ============================================================================
// ladybug.fingerprint(text) → fingerprint bytes
// ============================================================================

fn proc_fingerprint(args: &[Value]) -> Result<ProcedureResult> {
    let text = args.first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.fingerprint requires a string argument".into()))?;

    let fp = ContainerDto::random(siphash_string(text));

    let mut row = HashMap::new();
    row.insert("fingerprint".to_string(), Value::Bytes(fp.as_bytes().to_vec()));
    row.insert("popcount".to_string(), Value::Int(fp.popcount() as i64));
    row.insert("bits".to_string(), Value::Int(ContainerDto::BITS as i64));

    Ok(ProcedureResult {
        columns: vec!["fingerprint".to_string(), "popcount".to_string(), "bits".to_string()],
        rows: vec![row],
    })
}

// ============================================================================
// NARS procedures
// ============================================================================

/// ladybug.truth(frequency, confidence) → truth value
fn proc_truth(args: &[Value]) -> Result<ProcedureResult> {
    let freq = args.first()
        .and_then(|v| v.as_float())
        .ok_or_else(|| Error::ExecutionError("ladybug.truth requires (frequency, confidence)".into()))?;
    let conf = args.get(1)
        .and_then(|v| v.as_float())
        .ok_or_else(|| Error::ExecutionError("ladybug.truth requires (frequency, confidence)".into()))?;

    let expectation = conf * (freq - 0.5) + 0.5;
    let is_positive = expectation > 0.5;

    let mut row = HashMap::new();
    row.insert("frequency".to_string(), Value::Float(freq));
    row.insert("confidence".to_string(), Value::Float(conf));
    row.insert("expectation".to_string(), Value::Float(expectation));
    row.insert("positive".to_string(), Value::Bool(is_positive));

    Ok(ProcedureResult {
        columns: vec!["frequency".to_string(), "confidence".to_string(),
                      "expectation".to_string(), "positive".to_string()],
        rows: vec![row],
    })
}

/// ladybug.revision(f1, c1, f2, c2) → revised truth value
fn proc_revision(args: &[Value]) -> Result<ProcedureResult> {
    let f1 = args.first().and_then(|v| v.as_float()).unwrap_or(0.5);
    let c1 = args.get(1).and_then(|v| v.as_float()).unwrap_or(0.0);
    let f2 = args.get(2).and_then(|v| v.as_float()).unwrap_or(0.5);
    let c2 = args.get(3).and_then(|v| v.as_float()).unwrap_or(0.0);

    // NARS revision: combine independent evidence
    let horizon = 1.0_f64;
    let w1 = horizon * c1 / (1.0 - c1.min(1.0 - 1e-6));
    let w2 = horizon * c2 / (1.0 - c2.min(1.0 - 1e-6));

    let w1_pos = w1 * f1;
    let w1_neg = w1 * (1.0 - f1);
    let w2_pos = w2 * f2;
    let w2_neg = w2 * (1.0 - f2);

    let total_pos = w1_pos + w2_pos;
    let total_neg = w1_neg + w2_neg;
    let total = total_pos + total_neg;

    let freq = if total == 0.0 { 0.5 } else { total_pos / total };
    let conf = if total == 0.0 { 0.0 } else { total / (total + horizon) };

    let mut row = HashMap::new();
    row.insert("frequency".to_string(), Value::Float(freq));
    row.insert("confidence".to_string(), Value::Float(conf));
    row.insert("expectation".to_string(), Value::Float(conf * (freq - 0.5) + 0.5));

    Ok(ProcedureResult {
        columns: vec!["frequency".to_string(), "confidence".to_string(), "expectation".to_string()],
        rows: vec![row],
    })
}

/// ladybug.deduction(f1, c1, f2, c2) → deduced truth value
fn proc_deduction(args: &[Value]) -> Result<ProcedureResult> {
    let f1 = args.first().and_then(|v| v.as_float()).unwrap_or(0.5);
    let c1 = args.get(1).and_then(|v| v.as_float()).unwrap_or(0.0);
    let f2 = args.get(2).and_then(|v| v.as_float()).unwrap_or(0.5);
    let c2 = args.get(3).and_then(|v| v.as_float()).unwrap_or(0.0);

    // NARS deduction: A→B, B→C ⊢ A→C
    let f = f1 * f2;
    let c = c1 * c2 * f;

    let mut row = HashMap::new();
    row.insert("frequency".to_string(), Value::Float(f));
    row.insert("confidence".to_string(), Value::Float(c));

    Ok(ProcedureResult {
        columns: vec!["frequency".to_string(), "confidence".to_string()],
        rows: vec![row],
    })
}

/// ladybug.abduction(f1, c1, f2, c2) → abduced truth value
/// NARS abduction: A→B, B ⊢ A (weak inference)
fn proc_abduction(args: &[Value]) -> Result<ProcedureResult> {
    let f1 = args.first().and_then(|v| v.as_float()).unwrap_or(0.5);
    let c1 = args.get(1).and_then(|v| v.as_float()).unwrap_or(0.0);
    let f2 = args.get(2).and_then(|v| v.as_float()).unwrap_or(0.5);
    let c2 = args.get(3).and_then(|v| v.as_float()).unwrap_or(0.0);

    // NARS abduction: f = f2, c = f1 * c1 * c2 / (f1 * c1 * c2 + horizon)
    let horizon = 1.0_f64;
    let w = f1 * c1 * c2;
    let f = f2;
    let c = w / (w + horizon);

    let mut row = HashMap::new();
    row.insert("frequency".to_string(), Value::Float(f));
    row.insert("confidence".to_string(), Value::Float(c));
    row.insert("expectation".to_string(), Value::Float(c * (f - 0.5) + 0.5));

    Ok(ProcedureResult {
        columns: vec!["frequency".to_string(), "confidence".to_string(), "expectation".to_string()],
        rows: vec![row],
    })
}

/// ladybug.induction(f1, c1, f2, c2) → inducted truth value
/// NARS induction: A, A→B ⊢ generalise (A→B)
fn proc_induction(args: &[Value]) -> Result<ProcedureResult> {
    let f1 = args.first().and_then(|v| v.as_float()).unwrap_or(0.5);
    let c1 = args.get(1).and_then(|v| v.as_float()).unwrap_or(0.0);
    let f2 = args.get(2).and_then(|v| v.as_float()).unwrap_or(0.5);
    let c2 = args.get(3).and_then(|v| v.as_float()).unwrap_or(0.0);

    // NARS induction: f = f1, c = f2 * c1 * c2 / (f2 * c1 * c2 + horizon)
    let horizon = 1.0_f64;
    let w = f2 * c1 * c2;
    let f = f1;
    let c = w / (w + horizon);

    let mut row = HashMap::new();
    row.insert("frequency".to_string(), Value::Float(f));
    row.insert("confidence".to_string(), Value::Float(c));
    row.insert("expectation".to_string(), Value::Float(c * (f - 0.5) + 0.5));

    Ok(ProcedureResult {
        columns: vec!["frequency".to_string(), "confidence".to_string(), "expectation".to_string()],
        rows: vec![row],
    })
}

/// ladybug.crystallize(nodeId) → frozen status
fn proc_crystallize(args: &[Value]) -> Result<ProcedureResult> {
    let node_id = args.first()
        .and_then(|v| v.as_int())
        .ok_or_else(|| Error::ExecutionError("ladybug.crystallize requires a nodeId".into()))?;

    let mut row = HashMap::new();
    row.insert("nodeId".to_string(), Value::Int(node_id));
    row.insert("crystallized".to_string(), Value::Bool(true));
    row.insert("confidence".to_string(), Value::Float(0.99));

    Ok(ProcedureResult {
        columns: vec!["nodeId".to_string(), "crystallized".to_string(), "confidence".to_string()],
        rows: vec![row],
    })
}

/// ladybug.spine(rootLabel) → XOR-fold of all nodes with that label
fn proc_spine(
    args: &[Value],
    nodes: &HashMap<crate::model::NodeId, crate::model::Node>,
) -> Result<ProcedureResult> {
    let label = args.first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::ExecutionError("ladybug.spine requires a label string".into()))?;

    let fp = PropertyFingerprinter::cam();
    let mut spine = ContainerDto::zero();
    let mut count = 0u64;

    for node in nodes.values() {
        if node.labels.iter().any(|l| l == label) {
            let node_fp = fp.fingerprint(&node.properties);
            spine = spine.xor(&node_fp);
            count += 1;
        }
    }

    let mut row = HashMap::new();
    row.insert("label".to_string(), Value::from(label));
    row.insert("count".to_string(), Value::Int(count as i64));
    row.insert("spine".to_string(), Value::Bytes(spine.as_bytes().to_vec()));
    row.insert("popcount".to_string(), Value::Int(spine.popcount() as i64));

    Ok(ProcedureResult {
        columns: vec!["label".to_string(), "count".to_string(),
                      "spine".to_string(), "popcount".to_string()],
        rows: vec![row],
    })
}
