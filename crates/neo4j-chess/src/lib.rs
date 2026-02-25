//! Chess procedures for neo4j-rs.
//!
//! This crate isolates the chess/stonksfish dependencies so they don't
//! block compilation of the main neo4j-rs crate. The savant persona in
//! crewai-rust depends on this crate, not on neo4j-rs directly.
//!
//! Enable the `engine` feature for actual chess functionality.

#[cfg(feature = "engine")]
pub mod engine {
    // Chess engine integration goes here when stonksfish is available
}

/// Placeholder — chess procedures are available when the `engine` feature is enabled.
pub fn is_available() -> bool {
    cfg!(feature = "engine")
}
