//! Transaction management.

use serde::{Deserialize, Serialize};

/// Transaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TxMode {
    ReadOnly,
    ReadWrite,
}

/// Opaque transaction identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxId(pub u64);

/// Transaction trait that all backends must implement.
pub trait Transaction: Send + Sync {
    fn mode(&self) -> TxMode;
    fn id(&self) -> TxId;
}
