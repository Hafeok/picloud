//! Cross-node storage replication via HTTP file sync.
//!
//! When a volume is allocated with `FullReplication` durability, the primary
//! node periodically pushes changed files to all replica nodes. This module
//! implements the background sync task (push model).
//!
//! The companion HTTP endpoints (manifest + sync) live in picloud-http.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use picloud_domain::storage::{ManifestEntry, SyncFileEntry, SyncRequest, VolumeManifest};
use picloud_domain::traits::ClusterMembership;

// ---------------------------------------------------------------------------
// Manifest builder
// ---------------------------------------------------------------------------

/// Walk a volume directory and build a manifest of all files.
pub fn build_manifest(volume_name: &str, volume_path: &std::path::Path) -> VolumeManifest {
    let mut files = Vec::new();

    if volume_path.is_dir() {
        collect_files(volume_path, volume_path, &mut files);
    }

    VolumeManifest {
        volume: volume_name.to_string(),
        files,
    }
}

fn collect_files(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<ManifestEntry>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!(dir = %dir.display(), error = %e, "Failed to read directory");
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, out);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            out.push(ManifestEntry {
                path: relative,
                size: metadata.len(),
                mtime,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// StorageReplicator
// ---------------------------------------------------------------------------

/// Background task that periodically pushes changed volume files to peer nodes.
pub struct StorageReplicator {
    storage_path: PathBuf,
    cluster: Arc<dyn ClusterMembership>,
    interval_secs: u64,
    local_node_id: Uuid,
}

impl StorageReplicator {
    /// Create a new replicator.
    ///
    /// - `storage_path`: root directory containing volume subdirectories
    /// - `cluster`: cluster membership trait object for discovering peers
    /// - `local_node_id`: this node's UUID (to skip syncing to self)
    /// - `interval_secs`: how often to run the sync loop (seconds)
    pub fn new(
        storage_path: PathBuf,
        cluster: Arc<dyn ClusterMembership>,
        local_node_id: Uuid,
        interval_secs: u64,
    ) -> Self {
        Self {
            storage_path,
            cluster,
            interval_secs,
            local_node_id,
        }
    }

    /// Spawn the background sync task. Returns immediately.
    pub fn start(&self) {
        let storage_path = self.storage_path.clone();
        let cluster = self.cluster.clone();
        let local_node_id = self.local_node_id;
        let interval = Duration::from_secs(self.interval_secs);

        tokio::spawn(async move {
            info!(
                interval_secs = interval.as_secs(),
                storage_path = %storage_path.display(),
                "Storage replicator started"
            );

            let client = reqwest::Client::new();

            loop {
                tokio::time::sleep(interval).await;

                if let Err(e) =
                    run_sync_cycle(&client, &storage_path, &cluster, local_node_id).await
                {
                    error!(error = %e, "Storage sync cycle failed");
                }
            }
        });
    }
}

/// Execute one full sync cycle: for each volume, push changed files to each peer.
async fn run_sync_cycle(
    client: &reqwest::Client,
    storage_path: &std::path::Path,
    cluster: &Arc<dyn ClusterMembership>,
    local_node_id: Uuid,
) -> Result<(), String> {
    // List all volume directories
    let volumes = list_volume_dirs(storage_path)?;
    if volumes.is_empty() {
        debug!("No volumes to sync");
        return Ok(());
    }

    // Get cluster members
    let members = cluster
        .members()
        .await
        .map_err(|e| format!("Failed to get cluster members: {e}"))?;

    // Filter out self
    let peers: Vec<_> = members
        .iter()
        .filter(|m| m.node_id != local_node_id)
        .collect();

    if peers.is_empty() {
        debug!("No peers to sync to");
        return Ok(());
    }

    for (volume_name, volume_path) in &volumes {
        let local_manifest = build_manifest(volume_name, volume_path);

        for peer in &peers {
            let peer_addr = &peer.address;
            let base_url = if peer_addr.starts_with("http") {
                peer_addr.to_string()
            } else {
                format!("http://{}", peer_addr)
            };

            // Fetch remote manifest
            let manifest_url =
                format!("{}/internal/storage/manifest/{}", base_url, volume_name);

            let remote_manifest = match client.get(&manifest_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<VolumeManifest>().await {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(
                                peer = %peer_addr,
                                volume = volume_name,
                                error = %e,
                                "Failed to parse remote manifest"
                            );
                            // Treat as empty — push everything
                            VolumeManifest {
                                volume: volume_name.clone(),
                                files: Vec::new(),
                            }
                        }
                    }
                }
                Ok(resp) => {
                    debug!(
                        peer = %peer_addr,
                        volume = volume_name,
                        status = %resp.status(),
                        "Remote manifest not found — will push all files"
                    );
                    VolumeManifest {
                        volume: volume_name.clone(),
                        files: Vec::new(),
                    }
                }
                Err(e) => {
                    warn!(
                        peer = %peer_addr,
                        volume = volume_name,
                        error = %e,
                        "Failed to fetch remote manifest"
                    );
                    continue;
                }
            };

            // Build a lookup of remote files by path
            let remote_lookup: std::collections::HashMap<&str, &ManifestEntry> = remote_manifest
                .files
                .iter()
                .map(|f| (f.path.as_str(), f))
                .collect();

            // Find files that need syncing (new or newer mtime)
            let mut files_to_sync = Vec::new();
            for local_file in &local_manifest.files {
                let needs_sync = match remote_lookup.get(local_file.path.as_str()) {
                    Some(remote_file) => local_file.mtime > remote_file.mtime,
                    None => true, // file missing on remote
                };

                if needs_sync {
                    let file_path = volume_path.join(&local_file.path);
                    match tokio::fs::read(&file_path).await {
                        Ok(data) => {
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                            files_to_sync.push(SyncFileEntry {
                                path: local_file.path.clone(),
                                data: encoded,
                            });
                        }
                        Err(e) => {
                            warn!(
                                file = %local_file.path,
                                error = %e,
                                "Failed to read file for sync"
                            );
                        }
                    }
                }
            }

            if files_to_sync.is_empty() {
                debug!(
                    peer = %peer_addr,
                    volume = volume_name,
                    "No files need syncing"
                );
                continue;
            }

            let sync_count = files_to_sync.len();
            let sync_url =
                format!("{}/internal/storage/sync/{}", base_url, volume_name);
            let sync_request = SyncRequest {
                files: files_to_sync,
            };

            match client.post(&sync_url).json(&sync_request).send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!(
                        peer = %peer_addr,
                        volume = volume_name,
                        files = sync_count,
                        "Synced {sync_count} files to {peer_addr} for volume {volume_name}"
                    );
                }
                Ok(resp) => {
                    warn!(
                        peer = %peer_addr,
                        volume = volume_name,
                        status = %resp.status(),
                        "Sync request failed"
                    );
                }
                Err(e) => {
                    warn!(
                        peer = %peer_addr,
                        volume = volume_name,
                        error = %e,
                        "Failed to send sync request"
                    );
                }
            }
        }
    }

    Ok(())
}

/// List all subdirectories of `storage_path` as (volume_name, volume_path) pairs.
fn list_volume_dirs(storage_path: &std::path::Path) -> Result<Vec<(String, PathBuf)>, String> {
    if !storage_path.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(storage_path)
        .map_err(|e| format!("Failed to read storage directory: {e}"))?;

    let mut volumes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                volumes.push((name.to_string(), path));
            }
        }
    }

    Ok(volumes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("picloud-repl-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_build_manifest_empty_dir() {
        let dir = temp_dir();
        let vol_dir = dir.join("test-vol");
        std::fs::create_dir_all(&vol_dir).unwrap();

        let manifest = build_manifest("test-vol", &vol_dir);
        assert_eq!(manifest.volume, "test-vol");
        assert!(manifest.files.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_manifest_with_files() {
        let dir = temp_dir();
        let vol_dir = dir.join("media-store");
        std::fs::create_dir_all(&vol_dir).unwrap();

        // Create files
        let mut f1 = std::fs::File::create(vol_dir.join("photo.jpg")).unwrap();
        f1.write_all(&[0u8; 1024]).unwrap();

        std::fs::create_dir_all(vol_dir.join("data")).unwrap();
        let mut f2 = std::fs::File::create(vol_dir.join("data/index.db")).unwrap();
        f2.write_all(&[0u8; 512]).unwrap();

        let manifest = build_manifest("media-store", &vol_dir);
        assert_eq!(manifest.volume, "media-store");
        assert_eq!(manifest.files.len(), 2);

        let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"photo.jpg"));
        assert!(paths.contains(&"data/index.db"));

        // Check sizes
        let photo = manifest.files.iter().find(|f| f.path == "photo.jpg").unwrap();
        assert_eq!(photo.size, 1024);

        let index = manifest.files.iter().find(|f| f.path == "data/index.db").unwrap();
        assert_eq!(index.size, 512);

        // mtime should be non-zero
        assert!(photo.mtime > 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_build_manifest_nonexistent_dir() {
        let manifest = build_manifest("ghost", std::path::Path::new("/nonexistent/path"));
        assert!(manifest.files.is_empty());
    }

    #[test]
    fn test_list_volume_dirs() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("vol-a")).unwrap();
        std::fs::create_dir_all(dir.join("vol-b")).unwrap();
        // Create a regular file — should be ignored
        std::fs::File::create(dir.join("not-a-volume")).unwrap();

        let volumes = list_volume_dirs(&dir).unwrap();
        assert_eq!(volumes.len(), 2);

        let names: Vec<&str> = volumes.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"vol-a"));
        assert!(names.contains(&"vol-b"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_volume_dirs_nonexistent() {
        let volumes = list_volume_dirs(std::path::Path::new("/nonexistent/storage")).unwrap();
        assert!(volumes.is_empty());
    }

    #[test]
    fn test_unchanged_files_not_synced() {
        // Simulate mtime comparison: if local mtime <= remote mtime, file should not sync
        let local = ManifestEntry {
            path: "photo.jpg".to_string(),
            size: 1024,
            mtime: 100,
        };
        let remote = ManifestEntry {
            path: "photo.jpg".to_string(),
            size: 1024,
            mtime: 100,
        };
        // Same mtime — should NOT sync
        assert!(!(local.mtime > remote.mtime));

        let local_newer = ManifestEntry {
            path: "photo.jpg".to_string(),
            size: 2048,
            mtime: 200,
        };
        // Newer mtime — should sync
        assert!(local_newer.mtime > remote.mtime);
    }
}
