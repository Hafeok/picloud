/// Storage Types
///
/// Products declare storage intent — not implementation (ADR-024).
/// The platform translates intent into block allocation and replication.
/// MVP ships full-replication only.

use serde::{Deserialize, Serialize};

/// What a Product needs from storage — declared in the resource file.
/// The platform decides how to satisfy this intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageIntent {
    pub durability: DurabilityTier,
    pub performance: PerformanceTier,
}

impl Default for StorageIntent {
    fn default() -> Self {
        Self {
            durability: DurabilityTier::FullReplication,
            performance: PerformanceTier::Standard,
        }
    }
}

/// How many nodes hold a copy of this data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurabilityTier {
    /// Replicated to every available node — maximum durability
    /// This is the only tier available in Phase 1 (MVP)
    FullReplication,

    // --- Phase 4 tiers ---
    /// Replicated to a majority of nodes
    Quorum,
    /// Single node — no replication. Use for cache or ephemeral data.
    Local,
    /// No persistence — lost on workload restart
    None,
}

/// Read/write performance characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceTier {
    /// Balanced read/write — default
    Standard,

    // --- Phase 4 tiers ---
    /// Optimised for low-latency random read/write (databases)
    Fast,
    /// Optimised for sequential write, infrequent read
    Archive,
}
