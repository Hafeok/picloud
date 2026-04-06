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
use picloud_domain::storage::StorageIntent;
use picloud_domain::traits::{StorageBackend, VolumeHandle};

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

        info!(
            volume_iri = %volume_iri,
            size_gb = size_gb,
            device_path = %device_path,
            "Volume allocated"
        );

        Ok(VolumeHandle {
            volume_iri: volume_iri.clone(),
            device_path,
            replicated_to: vec![self.node_id],
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

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::storage::StorageIntent;

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
}
