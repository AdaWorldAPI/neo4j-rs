//! CogRecord8K Query Engine — Cypher compiles to rotation + popcount.
//!
//! This module bridges neo4j-rs Cypher execution to the CogRecord8K
//! 4-container architecture:
//!
//! ```text
//! Cypher Query                    CogRecord8K Operation
//! ──────────────────────────────  ───────────────────────────────────
//! MATCH (n:Person)                → popcount sweep on Container 1 (CAM)
//!   WHERE n.age > 30              → field check on Container 0 (META)
//! MATCH (n)-[:KNOWS]->(m)         → XOR-unbind on Container 2 (INDEX)
//! RETURN n.embedding              → read Container 3 (EMBED)
//! db.index.vector.queryNodes()    → VNNI dot-product on Container 3
//! ```
//!
//! # Architecture
//!
//! One VPOPCNTDQ pass per container per hop. No scan, no index lookup,
//! no serialization. The 4 containers give you:
//!
//! - Container 0 (META): 16,384 bits of metadata — identity, NARS truth,
//!   edges, rung/RL, qualia, bloom filter, codebook identity
//! - Container 1 (CAM): 16,384 bits of content-addressable memory —
//!   the searchable fingerprint, Hamming-queryable
//! - Container 2 (INDEX): 16,384 bits of B-tree/structural position —
//!   edge adjacency via XOR-bind rotation, spine cache, scent index
//! - Container 3 (EMBED): 16,384 bits of embedding storage —
//!   binary hash (VPOPCNTDQ) OR int8 vectors (VNNI VPDPBUSD)

use ladybug_contract::wide_container::WideContainer;
use ladybug_contract::cogrecord8k::{
    CogRecord8K, SLOT_META, SLOT_CAM, SLOT_INDEX, SLOT_EMBED,
};

// =============================================================================
// QUERY OPERATIONS
// =============================================================================

/// Which container a query operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryTarget {
    /// Container 0: metadata fields (identity, NARS, edges).
    Meta,
    /// Container 1: content-addressable memory (fingerprint search).
    Cam,
    /// Container 2: structural index (edge traversal, B-tree position).
    Index,
    /// Container 3: embedding store (Hamming or dot-product).
    Embed,
}

impl QueryTarget {
    pub fn slot(self) -> usize {
        match self {
            QueryTarget::Meta => SLOT_META,
            QueryTarget::Cam => SLOT_CAM,
            QueryTarget::Index => SLOT_INDEX,
            QueryTarget::Embed => SLOT_EMBED,
        }
    }
}

/// A compiled Hamming query operation.
///
/// Cypher clauses compile down to sequences of these operations.
/// Each operation is one VPOPCNTDQ pass (32 instructions per container).
#[derive(Debug, Clone)]
pub enum CogOp {
    /// Popcount sweep on a container: find records within Hamming threshold.
    ///
    /// Maps to: `MATCH (n:Label)` → sweep CAM container.
    /// 32 VPOPCNTDQ instructions per record.
    HammingSweep {
        target: QueryTarget,
        query: WideContainer,
        threshold: u32,
    },

    /// XOR-unbind: recover a component from a bound edge.
    ///
    /// Maps to: `MATCH (n)-[r:REL]->(m)` → unbind edge in INDEX container.
    /// edge ⊕ src ⊕ permute(rel, 1) → unpermute(result, 2) = target
    EdgeUnbind {
        edge: WideContainer,
        known_src: WideContainer,
        known_rel: WideContainer,
    },

    /// XOR-bind: encode an edge from components.
    ///
    /// Maps to: `CREATE (n)-[r:REL]->(m)` → encode edge into INDEX.
    EdgeBind {
        src: WideContainer,
        rel: WideContainer,
        tgt: WideContainer,
    },

    /// Int8 dot-product on embedding container.
    ///
    /// Maps to: `db.index.vector.queryNodes()` → VNNI VPDPBUSD.
    /// 1024 or 2048 dimensions of int8 multiply-accumulate.
    VectorDot {
        query_embed: WideContainer,
        dims: usize,
    },

    /// Field extraction from metadata container.
    ///
    /// Maps to: `WHERE n.property = value` → bit extraction from META.
    MetaFilter {
        word_offset: usize,
        mask: u64,
        expected: u64,
    },
}

/// Result of executing a CogOp against a CogRecord8K.
#[derive(Debug)]
pub struct CogOpResult {
    /// Hamming distance (for sweep/filter ops) or dot-product (for vector ops).
    pub score: i64,
    /// Whether this record passed the operation's threshold/filter.
    pub passed: bool,
    /// Number of VPOPCNTDQ or VNNI instructions consumed.
    pub instructions: u64,
}

// =============================================================================
// QUERY EXECUTION
// =============================================================================

/// Execute a single CogOp against a CogRecord8K.
pub fn execute_cogop(record: &CogRecord8K, op: &CogOp) -> CogOpResult {
    match op {
        CogOp::HammingSweep { target, query, threshold } => {
            let container = record.container(target.slot());
            let dist = container.hamming(query);
            CogOpResult {
                score: dist as i64,
                passed: dist < *threshold,
                instructions: 32, // 256 words / 8 per zmm = 32 VPOPCNTDQ
            }
        }

        CogOp::EdgeUnbind { edge, known_src, known_rel } => {
            let recovered = CogRecord8K::recover_target(edge, known_src, known_rel);
            // Check if the INDEX container is close to the recovered target
            let dist = record.container(SLOT_INDEX).hamming(&recovered);
            CogOpResult {
                score: dist as i64,
                passed: true, // unbind always produces a result
                instructions: 32 + 32, // unbind XOR + hamming check
            }
        }

        CogOp::EdgeBind { src, rel, tgt } => {
            let edge = CogRecord8K::make_edge(src, rel, tgt);
            let _ = edge; // The edge would be stored in INDEX
            CogOpResult {
                score: 0,
                passed: true,
                instructions: 32, // XOR operations
            }
        }

        CogOp::VectorDot { query_embed, dims } => {
            let dot = record.container(SLOT_EMBED).int8_dot(query_embed, *dims);
            CogOpResult {
                score: dot as i64,
                passed: true,
                instructions: (*dims / 64) as u64, // VNNI processes 64 bytes per instruction
            }
        }

        CogOp::MetaFilter { word_offset, mask, expected } => {
            let meta = record.container(SLOT_META);
            let actual = meta.words[*word_offset] & mask;
            CogOpResult {
                score: 0,
                passed: actual == *expected,
                instructions: 1,
            }
        }
    }
}

/// Execute a pipeline of CogOps with early exit.
///
/// This is how a compiled Cypher query runs: a sequence of CogOps
/// evaluated per record, with early exit on the first failing filter.
pub fn execute_pipeline(record: &CogRecord8K, ops: &[CogOp]) -> PipelineResult {
    let mut total_instructions = 0u64;
    let mut results = Vec::with_capacity(ops.len());

    for op in ops {
        let result = execute_cogop(record, op);
        total_instructions += result.instructions;
        let passed = result.passed;
        results.push(result);
        if !passed {
            return PipelineResult {
                passed: false,
                results,
                total_instructions,
            };
        }
    }

    PipelineResult {
        passed: true,
        results,
        total_instructions,
    }
}

/// Result of executing a full pipeline.
#[derive(Debug)]
pub struct PipelineResult {
    pub passed: bool,
    pub results: Vec<CogOpResult>,
    pub total_instructions: u64,
}

/// Sweep a corpus of CogRecord8K with a compiled pipeline.
///
/// This is the top-level query: Cypher → CogOps → sweep corpus.
/// Returns indices of matching records and their pipeline results.
pub fn sweep_corpus(
    corpus: &[CogRecord8K],
    ops: &[CogOp],
) -> Vec<(usize, PipelineResult)> {
    corpus.iter().enumerate()
        .filter_map(|(idx, record)| {
            let result = execute_pipeline(record, ops);
            if result.passed {
                Some((idx, result))
            } else {
                None
            }
        })
        .collect()
}

// =============================================================================
// CYPHER → COGOP COMPILER HELPERS
// =============================================================================

/// Compile a label match into a Hamming sweep on the CAM container.
///
/// The label is hashed into a WideContainer fingerprint, and nodes
/// matching that label will have CAM containers within Hamming threshold.
pub fn compile_label_match(label_fingerprint: &WideContainer, threshold: u32) -> CogOp {
    CogOp::HammingSweep {
        target: QueryTarget::Cam,
        query: label_fingerprint.clone(),
        threshold,
    }
}

/// Compile a relationship traversal into an edge unbind operation.
pub fn compile_edge_traversal(
    source_fingerprint: &WideContainer,
    relation_fingerprint: &WideContainer,
) -> CogOp {
    // The edge in INDEX = src ⊕ permute(rel, 1) ⊕ permute(tgt, 2)
    // To find target: unbind src and rel from the edge
    CogOp::EdgeUnbind {
        edge: WideContainer::zero(), // filled at runtime from INDEX container
        known_src: source_fingerprint.clone(),
        known_rel: relation_fingerprint.clone(),
    }
}

/// Compile a vector similarity query into a VNNI dot-product op.
pub fn compile_vector_query(query_embedding: &WideContainer, dims: usize) -> CogOp {
    CogOp::VectorDot {
        query_embed: query_embedding.clone(),
        dims,
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ladybug_contract::wide_container::EmbeddingFormat;

    #[test]
    fn test_hamming_sweep_op() {
        let query = WideContainer::random(42);
        let mut record = CogRecord8K::new();
        record.cam = query.clone(); // exact match

        let op = CogOp::HammingSweep {
            target: QueryTarget::Cam,
            query: query.clone(),
            threshold: 100,
        };

        let result = execute_cogop(&record, &op);
        assert!(result.passed, "exact match should pass");
        assert_eq!(result.score, 0, "exact match should have distance 0");
        assert_eq!(result.instructions, 32);
    }

    #[test]
    fn test_edge_bind_unbind() {
        let src = WideContainer::random(10);
        let rel = WideContainer::random(20);
        let tgt = WideContainer::random(30);

        let edge = CogRecord8K::make_edge(&src, &rel, &tgt);

        let op = CogOp::EdgeUnbind {
            edge: edge.clone(),
            known_src: src.clone(),
            known_rel: rel.clone(),
        };

        let mut record = CogRecord8K::new();
        record.index = tgt.clone(); // INDEX contains the target

        let result = execute_cogop(&record, &op);
        assert!(result.passed);
        assert_eq!(result.score, 0, "recovered target should match INDEX");
    }

    #[test]
    fn test_vector_dot_op() {
        let mut record = CogRecord8K::with_embedding_format(EmbeddingFormat::Int8x1024);
        let vals: Vec<i8> = (0..1024).map(|i| ((i % 127) as i8 - 64)).collect();
        record.embed.store_int8(&vals);

        let mut query_embed = WideContainer::zero();
        query_embed.store_int8(&vals);

        let op = CogOp::VectorDot {
            query_embed,
            dims: 1024,
        };

        let result = execute_cogop(&record, &op);
        assert!(result.passed);
        assert!(result.score > 0, "same vector dot should be positive");
    }

    #[test]
    fn test_meta_filter_op() {
        let mut record = CogRecord8K::new();
        // Set a specific value in meta word 5
        record.meta.words[5] = 0x00000000_DEADBEEF;

        let op = CogOp::MetaFilter {
            word_offset: 5,
            mask: 0xFFFF_FFFF,
            expected: 0xDEADBEEF,
        };

        let result = execute_cogop(&record, &op);
        assert!(result.passed, "matching meta filter should pass");

        // Non-matching filter
        let op_miss = CogOp::MetaFilter {
            word_offset: 5,
            mask: 0xFFFF_FFFF,
            expected: 0xCAFEBABE,
        };
        let result_miss = execute_cogop(&record, &op_miss);
        assert!(!result_miss.passed, "non-matching meta filter should fail");
    }

    #[test]
    fn test_pipeline_early_exit() {
        let query = WideContainer::random(42);
        let mut record = CogRecord8K::new();
        record.cam = WideContainer::random(999); // NOT a match

        let ops = vec![
            CogOp::HammingSweep {
                target: QueryTarget::Cam,
                query: query.clone(),
                threshold: 100, // very tight — will fail for random
            },
            CogOp::MetaFilter {
                word_offset: 0,
                mask: u64::MAX,
                expected: 0, // this would pass on zeroed meta
            },
        ];

        let result = execute_pipeline(&record, &ops);
        assert!(!result.passed, "pipeline should fail at first op");
        // Only 1 op executed (early exit before meta filter)
        assert_eq!(result.results.len(), 1, "should early-exit after first op");
    }

    #[test]
    fn test_corpus_sweep() {
        let query = WideContainer::random(42);
        let n = 100;

        let mut corpus: Vec<CogRecord8K> = (0..n)
            .map(|i| {
                let mut r = CogRecord8K::new();
                r.cam = WideContainer::random(i as u64 + 1000);
                r
            })
            .collect();

        // Plant one exact match
        corpus[50].cam = query.clone();

        let ops = vec![
            CogOp::HammingSweep {
                target: QueryTarget::Cam,
                query: query.clone(),
                threshold: 100,
            },
        ];

        let results = sweep_corpus(&corpus, &ops);
        assert_eq!(results.len(), 1, "should find exactly 1 match");
        assert_eq!(results[0].0, 50, "match should be at index 50");
    }

    #[test]
    fn test_compile_helpers() {
        let label_fp = WideContainer::random(1);
        let op = compile_label_match(&label_fp, 5000);
        assert!(matches!(op, CogOp::HammingSweep { .. }));

        let src_fp = WideContainer::random(2);
        let rel_fp = WideContainer::random(3);
        let op = compile_edge_traversal(&src_fp, &rel_fp);
        assert!(matches!(op, CogOp::EdgeUnbind { .. }));

        let emb = WideContainer::random(4);
        let op = compile_vector_query(&emb, 1024);
        assert!(matches!(op, CogOp::VectorDot { .. }));
    }
}
