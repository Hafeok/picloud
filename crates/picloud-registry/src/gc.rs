//! Garbage collection for the OCI registry (ADR-054).
//!
//! Mark-and-sweep: walks all manifests to find referenced blob digests,
//! then deletes any blobs not in the referenced set.
//! Safe to run online — blobs are marked live before any deletion.

use std::collections::HashSet;
use std::path::Path;

use tracing::{debug, info};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::traits::GarbageCollectionResult;

use crate::blob_store::BlobStore;

/// Garbage collector for the registry blob store.
pub struct GarbageCollector;

impl GarbageCollector {
    /// Run a mark-and-sweep garbage collection pass.
    ///
    /// 1. Walk all manifests to build the set of referenced digests
    /// 2. Walk all blobs
    /// 3. Delete blobs not in the referenced set
    pub async fn collect(
        repositories_root: &Path,
        blob_store: &BlobStore,
    ) -> Result<GarbageCollectionResult> {
        let start = std::time::Instant::now();

        // Phase 1: Mark — collect all digests referenced by manifests
        let referenced = Self::collect_referenced_digests(repositories_root).await?;
        debug!(referenced = referenced.len(), "GC mark phase complete");

        // Phase 2: Sweep — delete unreferenced blobs
        let all_digests = blob_store.list_digests().await?;
        let blobs_scanned = all_digests.len() as u64;
        let mut blobs_deleted = 0u64;
        let mut bytes_freed = 0u64;

        for digest in &all_digests {
            if !referenced.contains(digest) {
                // Get size before deleting
                if let Ok(data) = blob_store.get(digest).await {
                    bytes_freed += data.len() as u64;
                }
                if let Err(e) = blob_store.delete(digest).await {
                    debug!(digest = %digest, error = %e, "GC: failed to delete blob");
                    continue;
                }
                blobs_deleted += 1;
                debug!(digest = %digest, "GC: deleted unreferenced blob");
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        info!(
            scanned = blobs_scanned,
            deleted = blobs_deleted,
            freed_bytes = bytes_freed,
            duration_ms = duration_ms,
            "Registry garbage collection complete"
        );

        Ok(GarbageCollectionResult {
            blobs_scanned,
            blobs_deleted,
            bytes_freed,
            duration_ms,
        })
    }

    /// Walk all manifest files and extract referenced blob digests.
    async fn collect_referenced_digests(repositories_root: &Path) -> Result<HashSet<String>> {
        let mut referenced = HashSet::new();

        if !repositories_root.exists() {
            return Ok(referenced);
        }

        // Walk: repositories/{product}/{name}/manifests/{ref}
        let repos_dir = std::fs::read_dir(repositories_root).map_err(|e| {
            PiCloudError::RegistryError {
                reason: format!("Failed to read repositories dir: {e}"),
            }
        })?;

        for product_entry in repos_dir.flatten() {
            if !product_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let product_dir = std::fs::read_dir(product_entry.path());
            for repo_entry in product_dir.into_iter().flatten().flatten() {
                if !repo_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let manifests_dir = repo_entry.path().join("manifests");
                if !manifests_dir.exists() {
                    continue;
                }
                let manifests = std::fs::read_dir(&manifests_dir);
                for manifest_entry in manifests.into_iter().flatten().flatten() {
                    let path = manifest_entry.path();
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        // Parse manifest JSON to extract layer digests
                        Self::extract_digests_from_manifest(&content, &mut referenced);
                    }
                }
            }
        }

        Ok(referenced)
    }

    /// Extract blob digests from an OCI manifest JSON.
    fn extract_digests_from_manifest(manifest_json: &str, digests: &mut HashSet<String>) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(manifest_json) {
            // Config digest
            if let Some(config) = manifest.get("config") {
                if let Some(digest) = config.get("digest").and_then(|d| d.as_str()) {
                    digests.insert(digest.to_string());
                }
            }
            // Layer digests
            if let Some(layers) = manifest.get("layers").and_then(|l| l.as_array()) {
                for layer in layers {
                    if let Some(digest) = layer.get("digest").and_then(|d| d.as_str()) {
                        digests.insert(digest.to_string());
                    }
                }
            }
            // The manifest itself is also a blob (stored by its digest)
            if let Some(digest) = manifest.get("digest").and_then(|d| d.as_str()) {
                digests.insert(digest.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_digests_from_oci_manifest() {
        let manifest = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": "sha256:config111",
                "size": 1024
            },
            "layers": [
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": "sha256:layer111",
                    "size": 2048
                },
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": "sha256:layer222",
                    "size": 4096
                }
            ]
        }"#;

        let mut digests = HashSet::new();
        GarbageCollector::extract_digests_from_manifest(manifest, &mut digests);

        assert!(digests.contains("sha256:config111"));
        assert!(digests.contains("sha256:layer111"));
        assert!(digests.contains("sha256:layer222"));
        assert_eq!(digests.len(), 3);
    }
}
