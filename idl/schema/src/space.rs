use serde::{Deserialize, Serialize};

/// Account space model for init, realloc, and rent calculations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdlSpace {
    /// Discriminator size in bytes (typically 1 for Quasar accounts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<usize>,
    /// Minimum byte size (including discriminator for accounts).
    pub min: u64,
    /// Maximum byte size, or null for unbounded.
    pub max: Option<u64>,
    /// Human-readable formula string (documentation, not executable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}
