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
use picloud_domain::resources::VolumeType;
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
        volume_type: &VolumeType,
    ) -> Result<VolumeHandle> {
        let available = self.available_capacity_gb().await?;
        if size_gb > available {
            return Err(PiCloudError::InsufficientCapacity {
                requested_gb: size_gb,
                available_gb: available,
            });
        }

        let volume_name = Self::volume_name_from_iri(volume_iri);

        let device_path = match volume_type {
            VolumeType::Mounted => {
                // Mounted volumes: create a directory (existing Phase 1 behaviour)
                let volume_dir = self.base_path.join(&volume_name);
                std::fs::create_dir_all(&volume_dir).map_err(|e| {
                    PiCloudError::Internal(format!(
                        "Failed to create volume directory {}: {}",
                        volume_dir.display(),
                        e
                    ))
                })?;
                volume_dir.to_string_lossy().to_string()
            }
            VolumeType::RawBlock => {
                // Raw block devices: create a fixed-size file that simulates
                // a block device. On production NVMe hardware this would be a
                // real /dev/ node; in Phase 1 we use a regular file so tests
                // can read and write without root privileges.
                let block_dir = self.base_path.join("block");
                std::fs::create_dir_all(&block_dir).map_err(|e| {
                    PiCloudError::Internal(format!(
                        "Failed to create block device directory {}: {}",
                        block_dir.display(),
                        e
                    ))
                })?;
                let block_file = block_dir.join(&volume_name);
                // Allocate the file at the declared size (1 GB = 1 GiB for
                // simulation — we only write the first/last byte so the OS
                // can use a sparse file on filesystems that support it).
                let size_bytes = size_gb * 1024 * 1024 * 1024;
                let file = std::fs::File::create(&block_file).map_err(|e| {
                    PiCloudError::Internal(format!(
                        "Failed to create raw block file {}: {}",
                        block_file.display(),
                        e
                    ))
                })?;
                file.set_len(size_bytes).map_err(|e| {
                    PiCloudError::Internal(format!(
                        "Failed to set raw block file size {}: {}",
                        block_file.display(),
                        e
                    ))
                })?;
                block_file.to_string_lossy().to_string()
            }
        };

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
                    if path.is_dir() {
                        std::fs::remove_dir_all(&path).map_err(|e| {
                            PiCloudError::Internal(format!(
                                "Failed to remove volume directory {}: {}",
                                path.display(),
                                e
                            ))
                        })?;
                    } else {
                        std::fs::remove_file(&path).map_err(|e| {
                            PiCloudError::Internal(format!(
                                "Failed to remove raw block file {}: {}",
                                path.display(),
                                e
                            ))
                        })?;
                    }
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
        let timestamp = now.format("%Y-%m-%dT%H-%M-%S%.6fZ").to_string();
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

    async fn restore_snapshot_to_new_volume(
        &self,
        source_volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
        target_volume_iri: &ResourceIri,
    ) -> Result<()> {
        let source_name = Self::volume_name(source_volume_iri);
        let target_name = Self::volume_name(target_volume_iri);

        let snapshot_path = self
            .snapshot_dir(&source_name)
            .join(snapshot_timestamp);

        if !snapshot_path.exists() {
            return Err(PiCloudError::ResourceNotFound {
                iri: format!(
                    "snapshot {} for {}",
                    snapshot_timestamp, source_volume_iri
                ),
            });
        }

        let target_dir = self.base_path.join(&target_name);

        // Create the target volume directory and copy snapshot contents into it
        Self::copy_dir_all(&snapshot_path, &target_dir).map_err(|e| {
            PiCloudError::Internal(format!(
                "Failed to restore snapshot to new volume {}: {}",
                target_name, e
            ))
        })?;

        info!(
            source_volume = %source_volume_iri,
            target_volume = %target_volume_iri,
            snapshot = snapshot_timestamp,
            "Volume restored from snapshot to new volume"
        );

        Ok(())
    }

    async fn enforce_retention(
        &self,
        volume_iri: &ResourceIri,
        retention: &picloud_domain::storage::SnapshotRetention,
    ) -> Result<usize> {
        let mut snapshots = self.list_snapshots(volume_iri).await?;
        if snapshots.is_empty() {
            return Ok(0);
        }

        // Snapshots are sorted newest-first by list_snapshots
        // For Phase 1 we apply a simple policy: keep at most
        // retention.daily snapshots total (the most recent ones).
        // Weekly/monthly tiers will be implemented when the scheduler
        // categorises snapshots by age bracket.
        let max_keep = retention.daily as usize;
        if max_keep == 0 || snapshots.len() <= max_keep {
            return Ok(0);
        }

        // Sort newest-first (already done by list_snapshots, but be explicit)
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let to_delete = snapshots.split_off(max_keep);
        let deleted_count = to_delete.len();

        for snap in &to_delete {
            self.delete_snapshot(volume_iri, &snap.snapshot_path).await?;
        }

        info!(
            volume = %volume_iri,
            kept = max_keep,
            deleted = deleted_count,
            "Snapshot retention enforced"
        );

        Ok(deleted_count)
    }
}

/// Snapshot scheduler — checks whether a snapshot is due based on a
/// `SnapshotSchedule` and triggers creation + retention enforcement.
///
/// In production this runs as a background Tokio task. For testing it
/// exposes `run_once` which performs a single evaluation cycle.
pub struct SnapshotScheduler<S: SnapshotManager> {
    snapshot_manager: Arc<S>,
}

impl<S: SnapshotManager> SnapshotScheduler<S> {
    pub fn new(snapshot_manager: Arc<S>) -> Self {
        Self { snapshot_manager }
    }

    /// Determine whether a snapshot is due given the schedule and the most
    /// recent snapshot timestamp.
    pub fn is_due(
        schedule: &picloud_domain::storage::SnapshotSchedule,
        last_snapshot: Option<chrono::DateTime<chrono::Utc>>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let interval = match schedule {
            picloud_domain::storage::SnapshotSchedule::Hourly => chrono::Duration::hours(1),
            picloud_domain::storage::SnapshotSchedule::Daily => chrono::Duration::days(1),
            picloud_domain::storage::SnapshotSchedule::Weekly => chrono::Duration::weeks(1),
        };
        match last_snapshot {
            None => true, // never taken — due immediately
            Some(last) => now - last >= interval,
        }
    }

    /// Execute a single scheduling cycle for one volume:
    /// 1. Check if a snapshot is due.
    /// 2. Create a snapshot if due.
    /// 3. Enforce retention.
    ///
    /// Returns `Some(SnapshotInfo)` when a snapshot was created, `None` if
    /// no snapshot was due.
    pub async fn run_once(
        &self,
        volume_iri: &ResourceIri,
        config: &picloud_domain::storage::SnapshotConfig,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<SnapshotInfo>> {
        if !config.enabled {
            return Ok(None);
        }

        let existing = self.snapshot_manager.list_snapshots(volume_iri).await?;
        let last = existing.first().map(|s| s.created_at);

        if !Self::is_due(&config.schedule, last, now) {
            return Ok(None);
        }

        let info = self.snapshot_manager.create_snapshot(volume_iri).await?;

        // Enforce retention after creating a new snapshot
        self.snapshot_manager
            .enforce_retention(volume_iri, &config.retention)
            .await?;

        Ok(Some(info))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::iri::ResourceIri;
    use picloud_domain::resources::VolumeType;
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

        let handle = backend.allocate_volume(&iri, 30, &intent, &VolumeType::Mounted).await.unwrap();
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

        backend.allocate_volume(&iri, 25, &intent, &VolumeType::Mounted).await.unwrap();
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

        let result = backend.allocate_volume(&iri, 20, &intent, &VolumeType::Mounted).await;
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
            .allocate_volume(&make_volume_iri("v1"), 10, &intent, &VolumeType::Mounted)
            .await
            .unwrap();
        backend
            .allocate_volume(&make_volume_iri("v2"), 20, &intent, &VolumeType::Mounted)
            .await
            .unwrap();
        backend
            .allocate_volume(&make_volume_iri("v3"), 30, &intent, &VolumeType::Mounted)
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

        let handle = backend.allocate_volume(&iri, 10, &intent, &VolumeType::Mounted).await.unwrap();

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

        let handle = backend.allocate_volume(&iri, 10, &intent, &VolumeType::Mounted).await.unwrap();

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

        let handle = backend.allocate_volume(&iri, 10, &intent, &VolumeType::Mounted).await.unwrap();

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

        let handle = backend.allocate_volume(&iri, 10, &intent, &VolumeType::Mounted).await.unwrap();

        // Without cluster membership, only local node is listed
        assert_eq!(handle.replicated_to.len(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }

    // -----------------------------------------------------------------------
    // TC-243 — Raw block device attached to workload is readable and writable
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tc243_raw_block_device_attached_to_workload_is_readable_and_writable() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let iri = make_volume_iri("raw-vol");
        let intent = StorageIntent::default();

        // Allocate a raw block device volume (1 GB sparse file)
        let handle = backend
            .allocate_volume(&iri, 1, &intent, &VolumeType::RawBlock)
            .await
            .unwrap();

        // The device_path should point into the block/ subdirectory
        assert!(
            handle.device_path.contains("block/raw-vol"),
            "device_path should be a block file, got: {}",
            handle.device_path
        );

        let path = std::path::Path::new(&handle.device_path);
        assert!(path.exists(), "raw block file must exist on disk");
        assert!(path.is_file(), "raw block device must be a regular file");

        // File size should be 1 GiB (sparse)
        let meta = std::fs::metadata(path).unwrap();
        assert_eq!(
            meta.len(),
            1 * 1024 * 1024 * 1024,
            "raw block file should be 1 GiB"
        );

        // Write arbitrary data at the beginning of the block device
        use std::io::{Read, Seek, SeekFrom, Write};
        let test_data = b"PiCloud raw block device test payload";
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap();
            f.write_all(test_data).unwrap();
            f.flush().unwrap();
        }

        // Read it back and verify
        {
            let mut f = std::fs::File::open(path).unwrap();
            let mut buf = vec![0u8; test_data.len()];
            f.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, test_data, "read-back must match written data");
        }

        // Write at an arbitrary offset (simulating random I/O)
        let offset: u64 = 4096;
        let offset_data = b"offset-write-test";
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap();
            f.seek(SeekFrom::Start(offset)).unwrap();
            f.write_all(offset_data).unwrap();
            f.flush().unwrap();
        }

        // Read offset data back
        {
            let mut f = std::fs::File::open(path).unwrap();
            f.seek(SeekFrom::Start(offset)).unwrap();
            let mut buf = vec![0u8; offset_data.len()];
            f.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, offset_data, "offset read-back must match");
        }

        // Capacity should have decreased
        let available = backend.available_capacity_gb().await.unwrap();
        assert_eq!(available, 99, "allocating 1 GB should decrease capacity");

        // Clean up
        backend.delete_volume(&iri).await.unwrap();
        assert!(
            !path.exists(),
            "raw block file should be removed after delete"
        );
        assert_eq!(
            backend.available_capacity_gb().await.unwrap(),
            100,
            "capacity restored after deletion"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    // -----------------------------------------------------------------------
    // TC-300 — Block device exit — raw block device provisioned and accessible
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tc300_block_device_exit_raw_block_device_provisioned_and_accessible() {
        let base = temp_base_path();
        let local_id = Uuid::new_v4();
        let node2 = Uuid::new_v4();
        let node3 = Uuid::new_v4();
        let membership = Arc::new(MockClusterMembership::new(
            local_id,
            vec![local_id, node2, node3],
        ));
        let backend =
            LocalStorageBackend::with_cluster_membership(base.clone(), local_id, 200, membership);

        let intent = StorageIntent::default(); // FullReplication

        // --- Provision a raw block volume ---
        let iri = make_volume_iri("block-exit-vol");
        let handle = backend
            .allocate_volume(&iri, 2, &intent, &VolumeType::RawBlock)
            .await
            .unwrap();

        // Exit criterion 1: volume is provisioned — handle returned without error
        assert_eq!(handle.volume_iri, iri);

        // Exit criterion 2: device_path exists and is a file (Phase 1 simulation)
        let path = std::path::Path::new(&handle.device_path);
        assert!(path.exists(), "block device must be provisioned on disk");
        assert!(path.is_file(), "block device is a regular file in Phase 1");

        // Exit criterion 3: file is the declared size
        let meta = std::fs::metadata(path).unwrap();
        assert_eq!(meta.len(), 2 * 1024 * 1024 * 1024);

        // Exit criterion 4: replication targets populated for FullReplication
        assert_eq!(
            handle.replicated_to.len(),
            3,
            "FullReplication should list all cluster nodes"
        );

        // Exit criterion 5: the device is accessible (read + write)
        use std::io::{Read, Write};
        let payload = b"exit-criteria-validation";
        {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap();
            f.write_all(payload).unwrap();
        }
        {
            let mut f = std::fs::File::open(path).unwrap();
            let mut buf = vec![0u8; payload.len()];
            f.read_exact(&mut buf).unwrap();
            assert_eq!(&buf, payload);
        }

        // Exit criterion 6: a mounted volume still works alongside a raw block volume
        let mounted_iri = make_volume_iri("mounted-exit-vol");
        let mounted_handle = backend
            .allocate_volume(&mounted_iri, 5, &intent, &VolumeType::Mounted)
            .await
            .unwrap();
        let mounted_path = std::path::Path::new(&mounted_handle.device_path);
        assert!(mounted_path.is_dir(), "mounted volume must be a directory");

        // Exit criterion 7: capacity tracks both volume types correctly
        let available = backend.available_capacity_gb().await.unwrap();
        assert_eq!(available, 193, "200 - 2 (raw) - 5 (mounted) = 193");

        // Cleanup
        backend.delete_volume(&iri).await.unwrap();
        backend.delete_volume(&mounted_iri).await.unwrap();
        assert_eq!(backend.available_capacity_gb().await.unwrap(), 200);

        let _ = std::fs::remove_dir_all(&base);
    }

    // -----------------------------------------------------------------------
    // TC-246 — Volume snapshot created on schedule and restorable to new volume
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tc246_volume_snapshot_created_on_schedule_and_restorable_to_new_volume() {
        use picloud_domain::storage::{SnapshotConfig, SnapshotRetention, SnapshotSchedule};

        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let snap_mgr = Arc::new(LocalSnapshotManager::new(base.clone()));
        let scheduler = super::SnapshotScheduler::new(snap_mgr.clone());

        let volume_iri = make_volume_iri("sched-vol");
        let intent = StorageIntent::default();

        // --- Allocate a volume and write test data ---
        let handle = backend
            .allocate_volume(&volume_iri, 5, &intent, &VolumeType::Mounted)
            .await
            .unwrap();
        let data_file = std::path::PathBuf::from(&handle.device_path).join("data.txt");
        std::fs::write(&data_file, b"scheduled-snapshot-data").unwrap();

        // --- Configure a snapshot schedule (Hourly, keep 3 daily) ---
        let config = SnapshotConfig {
            enabled: true,
            schedule: SnapshotSchedule::Hourly,
            storage_secret: "test-nas-secret".to_string(),
            retention: SnapshotRetention {
                daily: 3,
                weekly: 2,
                monthly: 1,
            },
        };

        // --- First run: no previous snapshots → should create one ---
        let now = chrono::Utc::now();
        let result = scheduler.run_once(&volume_iri, &config, now).await.unwrap();
        assert!(result.is_some(), "first run should create a snapshot");
        let snap1 = result.unwrap();
        assert!(snap1.size_bytes > 0, "snapshot should have non-zero size");

        // --- Immediately after: not due yet (< 1 hour elapsed) ---
        let soon = now + chrono::Duration::minutes(30);
        let result2 = scheduler.run_once(&volume_iri, &config, soon).await.unwrap();
        assert!(result2.is_none(), "snapshot not due within the hour");

        // --- After 1 hour: should create another ---
        let one_hour_later = now + chrono::Duration::hours(1);
        let result3 = scheduler
            .run_once(&volume_iri, &config, one_hour_later)
            .await
            .unwrap();
        assert!(result3.is_some(), "snapshot due after 1 hour");

        // --- Verify snapshots are listed ---
        let listed = snap_mgr.list_snapshots(&volume_iri).await.unwrap();
        assert!(listed.len() >= 2, "at least 2 snapshots should exist");

        // --- Restore the first snapshot to a NEW volume ---
        let new_volume_iri = make_volume_iri("restored-vol");
        // Extract the timestamp portion from the first snapshot path
        let snap_timestamp = std::path::Path::new(&snap1.snapshot_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        snap_mgr
            .restore_snapshot_to_new_volume(&volume_iri, &snap_timestamp, &new_volume_iri)
            .await
            .unwrap();

        // --- Verify the restored volume contains the original data ---
        let restored_data_file = base.join("restored-vol").join("data.txt");
        assert!(
            restored_data_file.exists(),
            "restored volume should contain data.txt"
        );
        let restored_content = std::fs::read_to_string(&restored_data_file).unwrap();
        assert_eq!(
            restored_content, "scheduled-snapshot-data",
            "restored data must match original"
        );

        // --- Retention enforcement: create enough snapshots to exceed limit ---
        // We already have 2. Create 2 more to have 4 total (limit is 3).
        for _ in 0..2 {
            snap_mgr.create_snapshot(&volume_iri).await.unwrap();
        }
        let before_retention = snap_mgr.list_snapshots(&volume_iri).await.unwrap();
        assert!(
            before_retention.len() >= 4,
            "should have >= 4 snapshots before retention"
        );

        let deleted = snap_mgr
            .enforce_retention(&volume_iri, &config.retention)
            .await
            .unwrap();
        assert!(deleted > 0, "retention should have deleted old snapshots");

        let after_retention = snap_mgr.list_snapshots(&volume_iri).await.unwrap();
        assert_eq!(
            after_retention.len(),
            3,
            "should keep exactly 3 snapshots (daily limit)"
        );

        // --- Disabled config should be a no-op ---
        let disabled_config = SnapshotConfig {
            enabled: false,
            ..config.clone()
        };
        let disabled_result = scheduler
            .run_once(&volume_iri, &disabled_config, chrono::Utc::now())
            .await
            .unwrap();
        assert!(disabled_result.is_none(), "disabled config should not create snapshot");

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    // -----------------------------------------------------------------------
    // TC-303 — Volume snapshots exit — snapshot created, listed, and restored
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tc303_volume_snapshots_exit_snapshot_created_listed_and_restored() {
        let base = temp_base_path();
        let backend = make_backend(base.clone(), 100);
        let snap_mgr = LocalSnapshotManager::new(base.clone());

        let volume_iri = make_volume_iri("snap-exit-vol");
        let intent = StorageIntent::default();

        // --- 1. Allocate a mounted volume ---
        let handle = backend
            .allocate_volume(&volume_iri, 5, &intent, &VolumeType::Mounted)
            .await
            .unwrap();
        assert!(
            std::path::Path::new(&handle.device_path).is_dir(),
            "volume directory must exist"
        );

        // --- 2. Write known data to the volume ---
        let file1 = std::path::PathBuf::from(&handle.device_path).join("file1.txt");
        let file2 = std::path::PathBuf::from(&handle.device_path).join("file2.txt");
        std::fs::write(&file1, b"hello-snapshot").unwrap();
        std::fs::write(&file2, b"second-file-data").unwrap();

        // --- 3. Create a snapshot (exit criterion: snapshot created) ---
        let snap_info = snap_mgr.create_snapshot(&volume_iri).await.unwrap();
        assert_eq!(snap_info.volume_iri, volume_iri);
        assert!(snap_info.size_bytes > 0, "snapshot must have content");
        assert!(
            std::path::Path::new(&snap_info.snapshot_path).exists(),
            "snapshot directory must exist on disk"
        );

        // --- 4. List snapshots (exit criterion: snapshot listed) ---
        let listed = snap_mgr.list_snapshots(&volume_iri).await.unwrap();
        assert_eq!(listed.len(), 1, "exactly one snapshot should exist");
        assert_eq!(listed[0].snapshot_path, snap_info.snapshot_path);

        // --- 5. Mutate the volume data (simulates corruption / later writes) ---
        std::fs::write(&file1, b"overwritten-data").unwrap();
        std::fs::remove_file(&file2).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file1).unwrap(),
            "overwritten-data"
        );
        assert!(!file2.exists());

        // --- 6. Restore from snapshot (exit criterion: snapshot restored) ---
        let snap_timestamp = std::path::Path::new(&snap_info.snapshot_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        snap_mgr
            .restore_snapshot(&volume_iri, &snap_timestamp)
            .await
            .unwrap();

        // --- 7. Verify data matches the original snapshot ---
        let restored1 = std::fs::read_to_string(&file1).unwrap();
        assert_eq!(
            restored1, "hello-snapshot",
            "file1 must be restored to original content"
        );
        assert!(file2.exists(), "file2 must be restored");
        let restored2 = std::fs::read_to_string(&file2).unwrap();
        assert_eq!(
            restored2, "second-file-data",
            "file2 must be restored to original content"
        );

        // --- 8. Create a second snapshot and verify listing ---
        std::fs::write(&file1, b"new-data-after-restore").unwrap();
        let snap2 = snap_mgr.create_snapshot(&volume_iri).await.unwrap();
        let listed2 = snap_mgr.list_snapshots(&volume_iri).await.unwrap();
        assert_eq!(listed2.len(), 2, "two snapshots should exist");
        // Newest first
        assert_eq!(listed2[0].snapshot_path, snap2.snapshot_path);

        // --- 9. Restore to a new volume (exit criterion: restorable to different target) ---
        let new_vol_iri = make_volume_iri("snap-exit-restored");
        snap_mgr
            .restore_snapshot_to_new_volume(&volume_iri, &snap_timestamp, &new_vol_iri)
            .await
            .unwrap();
        let new_vol_file1 = base.join("snap-exit-restored").join("file1.txt");
        assert_eq!(
            std::fs::read_to_string(&new_vol_file1).unwrap(),
            "hello-snapshot",
            "new volume must contain original data from first snapshot"
        );

        // --- 10. Delete a snapshot and verify ---
        snap_mgr
            .delete_snapshot(&volume_iri, &snap_info.snapshot_path)
            .await
            .unwrap();
        let listed3 = snap_mgr.list_snapshots(&volume_iri).await.unwrap();
        assert_eq!(listed3.len(), 1, "one snapshot should remain after deletion");

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }
}
