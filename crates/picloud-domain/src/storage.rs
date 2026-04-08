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

// ---------------------------------------------------------------------------
// Replication sync types (shared between picloud-storage and picloud-http)
// ---------------------------------------------------------------------------

/// A single file entry in a volume manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Relative path within the volume directory.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Last modification time as Unix timestamp (seconds).
    pub mtime: i64,
}

/// The manifest for a single volume — lists every file it contains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeManifest {
    pub volume: String,
    pub files: Vec<ManifestEntry>,
}

/// A file to be synced to a remote node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileEntry {
    /// Relative path within the volume directory.
    pub path: String,
    /// Base64-encoded file content.
    pub data: String,
}

/// Request body for the POST /internal/storage/sync/:volume endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub files: Vec<SyncFileEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_intent_default_is_full_replication_standard() {
        let intent = StorageIntent::default();
        assert!(matches!(intent.durability, DurabilityTier::FullReplication));
        assert!(matches!(intent.performance, PerformanceTier::Standard));
    }

    #[test]
    fn durability_tier_serde_round_trip() {
        let tier = DurabilityTier::FullReplication;
        let json = serde_json::to_string(&tier).unwrap();
        let back: DurabilityTier = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, DurabilityTier::FullReplication));
    }

    #[test]
    fn performance_tier_serde_round_trip() {
        let tier = PerformanceTier::Standard;
        let json = serde_json::to_string(&tier).unwrap();
        let back: PerformanceTier = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, PerformanceTier::Standard));
    }

    #[test]
    fn storage_intent_serde_round_trip() {
        let intent = StorageIntent {
            durability: DurabilityTier::Quorum,
            performance: PerformanceTier::Fast,
        };
        let json = serde_json::to_string(&intent).unwrap();
        let back: StorageIntent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.durability, DurabilityTier::Quorum));
        assert!(matches!(back.performance, PerformanceTier::Fast));
    }
}
