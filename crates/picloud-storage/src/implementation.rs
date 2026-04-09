//! Local filesystem storage backend for Phase 1 (MVP).
//!
//! Simulates NVMe block storage using local directories.
//! Each volume gets a directory under `base_path/{volume_name}/`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::storage::{DurabilityTier, StorageIntent};
use picloud_domain::traits::{
    BackupInfo, ClusterMembership, SnapshotInfo, SnapshotManager, StorageBackend, VolumeHandle,
};

/// Internal record tracking an allocated volume.
#[derive(Debug, Clone)]
struct VolumeRecord {
    #[allow(dead_code)]
    volume_iri: String,
    size_gb: u64,
    device_path: String,
    #[allow(dead_code)]
    storage_intent: StorageIntent,
}

/// A storage backend that uses local filesystem directories to simulate
/// NVMe block storage. Suitable for Phase 1 / development.
pub struct LocalStorageBackend {
    base_path: PathBuf,
    node_id: Uuid,
    total_capacity_gb: u64,
    volumes: Arc<RwLock<HashMap<String, VolumeRecord>>>,
    #[allow(dead_code)]
    iri_builder: IriBuilder,
    /// Optional cluster membership info used to populate replication targets
    /// for volumes with `FullReplication` durability.
    cluster_membership: Option<Arc<dyn ClusterMembership>>,
}

impl LocalStorageBackend {
    /// Create a new local storage backend.
    ///
    /// - `base_path`: root directory where volume directories are created
    /// - `node_id`: this node's unique identifier
    /// - `total_capacity_gb`: total configured capacity in GB
    pub fn new(base_path: PathBuf, node_id: Uuid, total_capacity_gb: u64) -> Self {
        Self {
            base_path,
            node_id,
            total_capacity_gb,
            volumes: Arc::new(RwLock::new(HashMap::new())),
            iri_builder: IriBuilder::new(ClusterDomain::default()),
            cluster_membership: None,
        }
    }

    /// Create a new local storage backend with cluster membership awareness.
    ///
    /// When a volume is allocated with `FullReplication` durability, the
    /// returned `VolumeHandle` will list all known cluster node IDs in its
    /// `replicated_to` field (tracking that replication *should* happen).
    pub fn with_cluster_membership(
        base_path: PathBuf,
        node_id: Uuid,
        total_capacity_gb: u64,
        membership: Arc<dyn ClusterMembership>,
    ) -> Self {
        Self {
            base_path,
            node_id,
            total_capacity_gb,
            volumes: Arc::new(RwLock::new(HashMap::new())),
            iri_builder: IriBuilder::new(ClusterDomain::default()),
            cluster_membership: Some(membership),
        }
    }

    /// Extract a volume name from an IRI for use as a directory name.
    /// Takes the last path segment of the IRI.
    fn volume_name_from_iri(volume_iri: &ResourceIri) -> String {
        volume_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string()
    }

    /// Calculate currently allocated capacity in GB.
    async fn allocated_gb(&self) -> u64 {
        let volumes = self.volumes.read().await;
        volumes.values().map(|v| v.size_gb).sum()
    }
}

#[async_trait]
impl StorageBackend for LocalStorageBackend {
    async fn allocate_volume(
        &self,
        volume_iri: &ResourceIri,
        size_gb: u64,
        intent: &StorageIntent,
    ) -> Result<VolumeHandle> {
        let available = self.available_capacity_gb().await?;
        if size_gb > available {
            return Err(PiCloudError::InsufficientCapacity {
                requested_gb: size_gb,
                available_gb: available,
            });
        }

        let volume_name = Self::volume_name_from_iri(volume_iri);
        let volume_dir = self.base_path.join(&volume_name);

        std::fs::create_dir_all(&volume_dir).map_err(|e| {
            PiCloudError::Internal(format!(
                "Failed to create volume directory {}: {}",
                volume_dir.display(),
                e
            ))
        })?;

        let device_path = volume_dir.to_string_lossy().to_string();

        let record = VolumeRecord {
            volume_iri: volume_iri.as_str().to_string(),
            size_gb,
            device_path: device_path.clone(),
            storage_intent: intent.clone(),
        };

        {
            let mut volumes = self.volumes.write().await;
            volumes.insert(volume_iri.as_str().to_string(), record);
        }

        // Determine replication targets based on durability tier and cluster membership
        let replicated_to = match intent.durability {
            DurabilityTier::FullReplication => {
                if let Some(ref membership) = self.cluster_membership {
                    match membership.members().await {
                        Ok(members) => members.iter().map(|m| m.node_id).collect(),
                        Err(_) => {
                            warn!("Could not query cluster members; falling back to local node only");
                            vec![self.node_id]
                        }
                    }
                } else {
                    vec![self.node_id]
                }
            }
            DurabilityTier::Quorum => {
                if let Some(ref membership) = self.cluster_membership {
                    match membership.members().await {
                        Ok(members) => {
                            let quorum_size = (members.len() / 2) + 1;
                            members.iter().take(quorum_size).map(|m| m.node_id).collect()
                        }
                        Err(_) => vec![self.node_id],
                    }
                } else {
                    vec![self.node_id]
                }
            }
            DurabilityTier::Local | DurabilityTier::None => {
                vec![self.node_id]
            }
        };

        info!(
            volume_iri = %volume_iri,
            size_gb = size_gb,
            device_path = %device_path,
            replicated_to_count = replicated_to.len(),
            "Volume allocated"
        );

        Ok(VolumeHandle {
            volume_iri: volume_iri.clone(),
            device_path,
            replicated_to,
        })
    }

    async fn delete_volume(&self, volume_iri: &ResourceIri) -> Result<()> {
        let record = {
            let mut volumes = self.volumes.write().await;
            volumes.remove(volume_iri.as_str())
        };

        match record {
            Some(record) => {
                let path = PathBuf::from(&record.device_path);
                if path.exists() {
                    std::fs::remove_dir_all(&path).map_err(|e| {
                        PiCloudError::Internal(format!(
                            "Failed to remove volume directory {}: {}",
                            path.display(),
                            e
                        ))
                    })?;
                }

                info!(volume_iri = %volume_iri, "Volume deleted");
                Ok(())
            }
            None => {
                warn!(volume_iri = %volume_iri, "Attempted to delete unknown volume");
                Err(PiCloudError::ResourceNotFound {
                    iri: volume_iri.as_str().to_string(),
                })
            }
        }
    }

    async fn available_capacity_gb(&self) -> Result<u64> {
        let allocated = self.allocated_gb().await;
        Ok(self.total_capacity_gb.saturating_sub(allocated))
    }
}

/// Phase 1 snapshot manager — uses simple directory copies.
/// Snapshots are stored under `{base_path}/.snapshots/{volume_name}/{timestamp}/`.
/// No actual NFS/S3 transport — local filesystem paths only.
pub struct LocalSnapshotManager {
    base_path: PathBuf,
}

impl LocalSnapshotManager {
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Resolve the snapshot directory for a volume.
    fn snapshot_dir(&self, volume_name: &str) -> PathBuf {
        self.base_path
            .join(".snapshots")
            .join(volume_name)
    }

    /// Extract volume name from IRI (last path segment).
    fn volume_name(volume_iri: &ResourceIri) -> String {
        volume_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string()
    }

    /// Calculate directory size in bytes.
    fn dir_size(path: &std::path::Path) -> u64 {
        if !path.exists() {
            return 0;
        }
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| {
                        let meta = e.metadata().unwrap_or_else(|_| {
                            std::fs::metadata(e.path()).unwrap()
                        });
                        if meta.is_dir() {
                            Self::dir_size(&e.path())
                        } else {
                            meta.len()
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Recursively copy a directory.
    fn copy_dir_all(
        src: &std::path::Path,
        dst: &std::path::Path,
    ) -> std::result::Result<(), std::io::Error> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let dest_path = dst.join(entry.file_name());
            if file_type.is_dir() {
                Self::copy_dir_all(&entry.path(), &dest_path)?;
            } else {
                std::fs::copy(entry.path(), dest_path)?;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl SnapshotManager for LocalSnapshotManager {
    async fn create_snapshot(&self, volume_iri: &ResourceIri) -> Result<SnapshotInfo> {
        let volume_name = Self::volume_name(volume_iri);
        let volume_dir = self.base_path.join(&volume_name);

        if !volume_dir.exists() {
            return Err(PiCloudError::ResourceNotFound {
                iri: volume_iri.as_str().to_string(),
            });
        }

        let now = chrono::Utc::now();
        let timestamp = now.format("%Y-%m-%dT%H-%M-%SZ").to_string();
        let snapshot_path = self
            .snapshot_dir(&volume_name)
            .join(&timestamp);

        Self::copy_dir_all(&volume_dir, &snapshot_path).map_err(|e| {
            PiCloudError::Internal(format!(
                "Failed to create snapshot for {}: {}",
                volume_name, e
            ))
        })?;

        let size_bytes = Self::dir_size(&snapshot_path);

        info!(
            volume = %volume_iri,
            snapshot_path = %snapshot_path.display(),
            size_bytes = size_bytes,
            "Snapshot created"
        );

        Ok(SnapshotInfo {
            volume_iri: volume_iri.clone(),
            snapshot_path: snapshot_path.to_string_lossy().to_string(),
            size_bytes,
            created_at: now,
        })
    }

    async fn list_snapshots(&self, volume_iri: &ResourceIri) -> Result<Vec<SnapshotInfo>> {
        let volume_name = Self::volume_name(volume_iri);
        let snap_dir = self.snapshot_dir(&volume_name);

        if !snap_dir.exists() {
            return Ok(Vec::new());
        }

        let mut snapshots = Vec::new();
        let entries = std::fs::read_dir(&snap_dir).map_err(|e| {
            PiCloudError::Internal(format!("Failed to read snapshot dir: {}", e))
        })?;

        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                let path = entry.path();
                let size_bytes = Self::dir_size(&path);
                let created_at = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.created().ok())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t))
                    .unwrap_or_else(chrono::Utc::now);

                snapshots.push(SnapshotInfo {
                    volume_iri: volume_iri.clone(),
                    snapshot_path: path.to_string_lossy().to_string(),
                    size_bytes,
                    created_at,
                });
            }
        }

        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(snapshots)
    }

    async fn restore_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
    ) -> Result<()> {
        let volume_name = Self::volume_name(volume_iri);
        let snapshot_path = self
            .snapshot_dir(&volume_name)
            .join(snapshot_timestamp);

        if !snapshot_path.exists() {
            return Err(PiCloudError::ResourceNotFound {
                iri: format!("snapshot {} for {}", snapshot_timestamp, volume_iri),
            });
        }

        let volume_dir = self.base_path.join(&volume_name);

        // Remove current volume contents
        if volume_dir.exists() {
            std::fs::remove_dir_all(&volume_dir).map_err(|e| {
                PiCloudError::Internal(format!(
                    "Failed to clear volume {}: {}",
                    volume_name, e
                ))
            })?;
        }

        // Copy snapshot back to volume dir
        Self::copy_dir_all(&snapshot_path, &volume_dir).map_err(|e| {
            PiCloudError::Internal(format!(
                "Failed to restore snapshot for {}: {}",
                volume_name, e
            ))
        })?;

        info!(
            volume = %volume_iri,
            snapshot = snapshot_timestamp,
            "Volume restored from snapshot"
        );

        Ok(())
    }

    async fn delete_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_path: &str,
    ) -> Result<()> {
        let path = PathBuf::from(snapshot_path);
        if !path.exists() {
            return Err(PiCloudError::ResourceNotFound {
                iri: format!("snapshot {} for {}", snapshot_path, volume_iri),
            });
        }

        std::fs::remove_dir_all(&path).map_err(|e| {
            PiCloudError::Internal(format!(
                "Failed to delete snapshot: {}",
                e
            ))
        })?;

        info!(volume = %volume_iri, snapshot_path = snapshot_path, "Snapshot deleted");
        Ok(())
    }

    async fn list_backups(&self, _volume_iri: &ResourceIri) -> Result<Vec<BackupInfo>> {
        // Phase 1: no actual offsite backup transport — return empty list
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::iri::ResourceIri;
    use picloud_domain::storage::StorageIntent;
    use picloud_domain::traits::{ClusterMembership, NodeInfo};

    fn temp_base_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("picloud-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("failed to create temp dir");
        dir
    }

    fn make_volume_iri(name: &str) -> ResourceIri {
        let builder = IriBuilder::new(ClusterDomain::default());
        builder.resource("test-product", "volumes", name)
    }

    fn make_backend(base_path: PathBuf, capacity_gb: u64) -> LocalStorageBackend {
        LocalStorageBackend::new(base_path, Uuid::new_v4(), capacity_gb)
    }

    /// A mock ClusterMembership that returns a fixed set of node IDs.
    struct MockClusterMembership {
        local_node_id: Uuid,
        nodes: Vec<Uuid>,
    }

    impl MockClusterMembership {
        fn new(local_id: Uuid, nodes: Vec<Uuid>) -> Self {
            Self {
                local_node_id: local_id,
                nodes,
            }
        }
    }

    #[async_trait]
    impl ClusterMembership for MockClusterMembership {
        async fn is_leader(&self) -> bool {
            true
        }
        async fn leader_id(&self) -> picloud_domain::error::Result<Uuid> {
            Ok(self.local_node_id)
        }
        async fn members(&self) -> picloud_domain::error::Result<Vec<NodeInfo>> {
            let builder = IriBuilder::new(ClusterDomain::default());
            Ok(self
                .nodes
                .iter()
                .map(|id| NodeInfo {
                    node_id: *id,
                    node_iri: builder.resource("cluster", "nodes", &id.to_string()),
                    address: format!("10.0.0.{}", id.as_bytes()[0]),
                    is_leader: *id == self.local_node_id,
                })
                .collect())
        }
        async fn local_node_id(&self) -> Uuid {
            self.local_node_id
        }
    }

    #[tokio::test]
    async fn allocate_volume_decreases_capacity() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let iri = make_volume_iri("vol-1");
        let intent = StorageIntent::default();

        let handle = backend.allocate_volume(&iri, 30, &intent).await.unwrap();
        assert_eq!(handle.volume_iri, iri);
        assert!(handle.device_path.contains("vol-1"));
        assert_eq!(handle.replicated_to.len(), 1);

        let available = backend.available_capacity_gb().await.unwrap();
        assert_eq!(available, 70);

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn delete_volume_restores_capacity() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let iri = make_volume_iri("vol-2");
        let intent = StorageIntent::default();

        backend.allocate_volume(&iri, 25, &intent).await.unwrap();
        assert_eq!(backend.available_capacity_gb().await.unwrap(), 75);

        backend.delete_volume(&iri).await.unwrap();
        assert_eq!(backend.available_capacity_gb().await.unwrap(), 100);

        // Directory should be removed
        assert!(!base.join("vol-2").exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn allocate_insufficient_capacity_returns_error() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 10);
        let iri = make_volume_iri("big-vol");
        let intent = StorageIntent::default();

        let result = backend.allocate_volume(&iri, 20, &intent).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            PiCloudError::InsufficientCapacity {
                requested_gb,
                available_gb,
            } => {
                assert_eq!(requested_gb, 20);
                assert_eq!(available_gb, 10);
            }
            other => panic!("Expected InsufficientCapacity, got: {:?}", other),
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn available_capacity_tracks_multiple_volumes() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let intent = StorageIntent::default();

        backend
            .allocate_volume(&make_volume_iri("v1"), 10, &intent)
            .await
            .unwrap();
        backend
            .allocate_volume(&make_volume_iri("v2"), 20, &intent)
            .await
            .unwrap();
        backend
            .allocate_volume(&make_volume_iri("v3"), 30, &intent)
            .await
            .unwrap();

        assert_eq!(backend.available_capacity_gb().await.unwrap(), 40);

        backend.delete_volume(&make_volume_iri("v2")).await.unwrap();
        assert_eq!(backend.available_capacity_gb().await.unwrap(), 60);

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn delete_nonexistent_volume_returns_not_found() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let iri = make_volume_iri("ghost");

        let result = backend.delete_volume(&iri).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            PiCloudError::ResourceNotFound { iri } => {
                assert!(iri.contains("ghost"));
            }
            other => panic!("Expected ResourceNotFound, got: {:?}", other),
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn full_replication_lists_all_cluster_nodes() {
        let base = temp_base_path();
        let local_id = Uuid::new_v4();
        let node2 = Uuid::new_v4();
        let node3 = Uuid::new_v4();
        let membership = Arc::new(MockClusterMembership::new(
            local_id,
            vec![local_id, node2, node3],
        ));
        let backend =
            LocalStorageBackend::with_cluster_membership(base.clone(), local_id, 100, membership);

        let iri = make_volume_iri("replicated-vol");
        let intent = StorageIntent::default(); // FullReplication

        let handle = backend.allocate_volume(&iri, 10, &intent).await.unwrap();

        // Should list all 3 cluster nodes
        assert_eq!(handle.replicated_to.len(), 3);
        assert!(handle.replicated_to.contains(&local_id));
        assert!(handle.replicated_to.contains(&node2));
        assert!(handle.replicated_to.contains(&node3));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn quorum_replication_lists_majority_of_nodes() {
        let base = temp_base_path();
        let local_id = Uuid::new_v4();
        let node2 = Uuid::new_v4();
        let node3 = Uuid::new_v4();
        let node4 = Uuid::new_v4();
        let node5 = Uuid::new_v4();
        let membership = Arc::new(MockClusterMembership::new(
            local_id,
            vec![local_id, node2, node3, node4, node5],
        ));
        let backend =
            LocalStorageBackend::with_cluster_membership(base.clone(), local_id, 100, membership);

        let iri = make_volume_iri("quorum-vol");
        let intent = StorageIntent {
            durability: DurabilityTier::Quorum,
            performance: picloud_domain::storage::PerformanceTier::Standard,
            snapshots: None,
            offsite: None,
        };

        let handle = backend.allocate_volume(&iri, 10, &intent).await.unwrap();

        // Quorum of 5 nodes = 3 (5/2 + 1)
        assert_eq!(handle.replicated_to.len(), 3);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn local_durability_only_lists_local_node() {
        let base = temp_base_path();
        let local_id = Uuid::new_v4();
        let node2 = Uuid::new_v4();
        let membership = Arc::new(MockClusterMembership::new(
            local_id,
            vec![local_id, node2],
        ));
        let backend =
            LocalStorageBackend::with_cluster_membership(base.clone(), local_id, 100, membership);

        let iri = make_volume_iri("local-vol");
        let intent = StorageIntent {
            durability: DurabilityTier::Local,
            performance: picloud_domain::storage::PerformanceTier::Standard,
            snapshots: None,
            offsite: None,
        };

        let handle = backend.allocate_volume(&iri, 10, &intent).await.unwrap();

        // Local durability = only this node
        assert_eq!(handle.replicated_to.len(), 1);
        assert_eq!(handle.replicated_to[0], local_id);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn no_membership_falls_back_to_local_node() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);

        let iri = make_volume_iri("no-membership-vol");
        let intent = StorageIntent::default(); // FullReplication

        let handle = backend.allocate_volume(&iri, 10, &intent).await.unwrap();

        // Without cluster membership, only local node is listed
        assert_eq!(handle.replicated_to.len(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }
}
