//! Property → Container fingerprinting.
//!
//! Converts Neo4j node properties into 8192-bit content containers
//! suitable for Hamming distance comparison. Keys are sorted before
//! fingerprinting to ensure deterministic output regardless of HashMap
//! iteration order (addresses wiring-plan gotcha W-3).

use std::collections::HashMap;

use crate::model::Value;

/// SipHash-style string → u64 seed, used to bootstrap deterministic container generation.
pub(crate) fn siphash_string(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ============================================================================
// Container — 8192-bit (128 × u64) fingerprint
// ============================================================================

/// 8192-bit binary container for HDC/VSA operations.
///
/// Mirrors `ladybug_contract::Container` layout. `#[repr(C, align(64))]`
/// ensures binary compatibility — a `ContainerDto` can be transmuted to
/// a ladybug `Container` without copying.
#[derive(Clone, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct ContainerDto {
    pub words: [u64; 128],
}

impl ContainerDto {
    pub const BITS: usize = 8_192;
    pub const WORDS: usize = 128;
    pub const BYTES: usize = 1024;

    /// All-zero container.
    #[inline]
    pub fn zero() -> Self {
        Self { words: [0u64; 128] }
    }

    /// Deterministic pseudo-random container from seed (SplitMix64 + xorshift64).
    /// Matches `ladybug_contract::Container::random()` exactly.
    pub fn random(seed: u64) -> Self {
        let mut z = seed.wrapping_add(0x9e3779b97f4a7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        let mut state = (z ^ (z >> 31)) | 1;
        let mut words = [0u64; 128];
        for w in words.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *w = state;
        }
        Self { words }
    }

    /// XOR binding — the fundamental associative operation.
    #[inline]
    pub fn xor(&self, other: &ContainerDto) -> ContainerDto {
        let mut result = ContainerDto::zero();
        for i in 0..Self::WORDS {
            result.words[i] = self.words[i] ^ other.words[i];
        }
        result
    }

    /// Hamming distance (number of differing bits).
    #[inline]
    pub fn hamming(&self, other: &ContainerDto) -> u32 {
        let mut dist = 0u32;
        for i in 0..Self::WORDS {
            dist += (self.words[i] ^ other.words[i]).count_ones();
        }
        dist
    }

    /// Cosine-like similarity: 1.0 − hamming / 8192.
    #[inline]
    pub fn similarity(&self, other: &ContainerDto) -> f32 {
        1.0 - (self.hamming(other) as f32 / Self::BITS as f32)
    }

    /// Population count (number of set bits).
    #[inline]
    pub fn popcount(&self) -> u32 {
        self.words.iter().map(|w| w.count_ones()).sum()
    }

    /// True if all bits are zero.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    /// Raw bytes view (1024 bytes, cache-aligned).
    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.words.as_ptr() as *const u8,
                Self::BYTES,
            )
        }
    }
}

impl std::fmt::Debug for ContainerDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Container(pop={}, 0x{:016x}..)", self.popcount(), self.words[0])
    }
}

// ============================================================================
// Fingerprint mode
// ============================================================================

/// How to convert properties into a content container.
#[derive(Debug, Clone)]
pub enum FingerprintMode {
    /// Full CAM: record IS its fingerprint. Default and most common.
    Cam,

    /// Bitpacked: content container for distance-only comparison.
    /// Used when properties are purely structural.
    Bitpacked,

    /// Hybrid: CAM fingerprint + external Jina/OpenAI float vector.
    /// The container stores a CAM proxy; the real vector lives in Lance.
    Hybrid {
        /// Endpoint for external embedding service.
        embedding_endpoint: String,
    },
}

// ============================================================================
// PropertyFingerprinter
// ============================================================================

/// Converts Neo4j property maps into 8192-bit containers.
///
/// Properties are sorted by key before fingerprinting to ensure
/// deterministic output. Each key-value pair is hashed and XOR-bound.
pub struct PropertyFingerprinter {
    pub mode: FingerprintMode,
}

impl PropertyFingerprinter {
    pub fn new(mode: FingerprintMode) -> Self {
        Self { mode }
    }

    /// Default CAM fingerprinter.
    pub fn cam() -> Self {
        Self::new(FingerprintMode::Cam)
    }

    /// Fingerprint a property map into a container.
    ///
    /// Algorithm:
    /// 1. Sort keys alphabetically (deterministic iteration order)
    /// 2. For each (key, value): hash key → Container, hash value → Container
    /// 3. XOR-bind key ⊕ value per pair
    /// 4. XOR-fold all pairs into final container
    pub fn fingerprint(&self, properties: &HashMap<String, Value>) -> ContainerDto {
        if properties.is_empty() {
            return ContainerDto::zero();
        }

        let mut keys: Vec<&String> = properties.keys().collect();
        keys.sort();

        let mut result = ContainerDto::zero();
        for key in keys {
            let value = &properties[key];
            let key_fp = ContainerDto::random(siphash_string(key));
            let val_fp = ContainerDto::random(siphash_string(&value_to_hash_string(value)));
            let pair_fp = key_fp.xor(&val_fp);
            result = result.xor(&pair_fp);
        }
        result
    }

    /// Fingerprint a single string value (for labels, rel_types, etc.).
    pub fn fingerprint_string(s: &str) -> ContainerDto {
        ContainerDto::random(siphash_string(s))
    }
}

// ============================================================================
// Label binding
// ============================================================================

/// XOR-bind all labels into a single 64-bit hash.
///
/// Multi-label nodes (e.g., `Stakeholder:TechCompany:AIDeveloper`)
/// XOR all label hashes. Stored in MetaView W3 (label_hash).
///
/// Labels are sorted before binding for determinism.
pub fn bind_labels(labels: &[String]) -> u64 {
    let mut sorted: Vec<&String> = labels.iter().collect();
    sorted.sort();

    let mut hash = 0u64;
    for label in sorted {
        hash ^= siphash_string(label);
    }
    hash
}

/// XOR-bind labels from string slices.
pub fn bind_label_strs(labels: &[&str]) -> u64 {
    let mut sorted: Vec<&str> = labels.to_vec();
    sorted.sort();

    let mut hash = 0u64;
    for label in sorted {
        hash ^= siphash_string(label);
    }
    hash
}

// ============================================================================
// Value → hashable string
// ============================================================================

/// Convert a Value to a deterministic string for hashing.
///
/// NaN floats are mapped to the string "NaN" (not skipped) to preserve
/// the information that a NaN was present.
fn value_to_hash_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            if f.is_nan() {
                "NaN".to_string()
            } else {
                // Use {:?} for full precision round-trip
                format!("{f:?}")
            }
        }
        Value::String(s) => s.clone(),
        Value::Bytes(b) => format!("bytes:{}", b.len()),
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(value_to_hash_string).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Map(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys.iter()
                .map(|k| format!("{}:{}", k, value_to_hash_string(&m[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        // Temporal and spatial types: use Display impl
        other => format!("{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_random_deterministic() {
        let a = ContainerDto::random(42);
        let b = ContainerDto::random(42);
        assert_eq!(a, b);
        assert!(!a.is_zero());
    }

    #[test]
    fn test_container_xor_self_is_zero() {
        let a = ContainerDto::random(123);
        let result = a.xor(&a);
        assert!(result.is_zero());
    }

    #[test]
    fn test_container_similarity() {
        let a = ContainerDto::random(1);
        let b = ContainerDto::random(2);

        // Self-similarity = 1.0
        assert!((a.similarity(&a) - 1.0).abs() < f32::EPSILON);

        // Random containers ~0.5 similarity
        let sim = a.similarity(&b);
        assert!(sim > 0.45 && sim < 0.55, "sim={sim}");
    }

    #[test]
    fn test_fingerprint_deterministic_order() {
        let mut props1 = HashMap::new();
        props1.insert("name".to_string(), Value::from("Ada"));
        props1.insert("age".to_string(), Value::from(30));

        let mut props2 = HashMap::new();
        props2.insert("age".to_string(), Value::from(30));
        props2.insert("name".to_string(), Value::from("Ada"));

        let fp = PropertyFingerprinter::cam();
        assert_eq!(fp.fingerprint(&props1), fp.fingerprint(&props2));
    }

    #[test]
    fn test_bind_labels_deterministic() {
        let labels1 = vec!["Person".to_string(), "Developer".to_string()];
        let labels2 = vec!["Developer".to_string(), "Person".to_string()];
        assert_eq!(bind_labels(&labels1), bind_labels(&labels2));
    }

    #[test]
    fn test_bind_labels_single() {
        let labels = vec!["Person".to_string()];
        let hash = bind_labels(&labels);
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_empty_properties_fingerprint() {
        let fp = PropertyFingerprinter::cam();
        let result = fp.fingerprint(&HashMap::new());
        assert!(result.is_zero());
    }
}
