//! LocalRegistryBackend — implements RegistryBackend from picloud-domain.
//!
//! Filesystem-based OCI registry. Blobs are content-addressed,
//! manifests are stored per-repository, tags point to digests.
//!
//! Layout:
//!   {root}/blobs/sha256/{hex}           — content-addressed blobs
//!   {root}/repositories/{product}/{name}/manifests/{ref}  — manifest JSON
//!   {root}/uploads/{uuid}               — in-progress blob uploads

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tracing::{debug, info};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::traits::{GarbageCollectionResult, RegistryBackend, RegistryStats};

use crate::blob_store::BlobStore;
use crate::gc::GarbageCollector;

/// Filesystem-backed OCI registry.
pub struct LocalRegistryBackend {
    root: PathBuf,
    blob_store: Arc<BlobStore>,
}

impl LocalRegistryBackend {
    /// Create a new registry at the given root path.
    pub fn new(root: &Path) -> Result<Self> {
        let blobs_dir = root.join("blobs");
        let repos_dir = root.join("repositories");
        let uploads_dir = root.join("uploads");

        std::fs::create_dir_all(&blobs_dir).map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to create blobs dir: {e}"),
        })?;
        std::fs::create_dir_all(&repos_dir).map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to create repositories dir: {e}"),
        })?;
        std::fs::create_dir_all(&uploads_dir).map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to create uploads dir: {e}"),
        })?;

        let blob_store = Arc::new(BlobStore::new(&blobs_dir)?);

        info!(root = %root.display(), "Registry storage initialized");

        Ok(Self {
            root: root.to_path_buf(),
            blob_store,
        })
    }

    fn repositories_dir(&self) -> PathBuf {
        self.root.join("repositories")
    }

    fn manifest_path(&self, repository: &str, reference: &str) -> PathBuf {
        self.repositories_dir()
            .join(repository)
            .join("manifests")
            .join(sanitize_reference(reference))
    }

    fn manifest_content_type_path(&self, repository: &str, reference: &str) -> PathBuf {
        self.repositories_dir()
            .join(repository)
            .join("manifests")
            .join(format!("{}.content-type", sanitize_reference(reference)))
    }

    fn uploads_dir(&self) -> PathBuf {
        self.root.join("uploads")
    }
}

#[async_trait]
impl RegistryBackend for LocalRegistryBackend {
    async fn put_blob(&self, digest: &str, data: Vec<u8>) -> Result<()> {
        self.blob_store.put(digest, &data).await
    }

    async fn get_blob(&self, digest: &str) -> Result<Vec<u8>> {
        self.blob_store.get(digest).await
    }

    async fn blob_exists(&self, digest: &str) -> Result<bool> {
        Ok(self.blob_store.exists(digest).await)
    }

    async fn delete_blob(&self, digest: &str) -> Result<()> {
        self.blob_store.delete(digest).await
    }

    async fn put_manifest(
        &self,
        repository: &str,
        reference: &str,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<String> {
        // Compute digest of the manifest
        let digest = format!("sha256:{}", hex_encode(Sha256::digest(&data)));

        // Store the manifest by its reference (tag or digest)
        let manifest_path = self.manifest_path(repository, reference);
        let ct_path = self.manifest_content_type_path(repository, reference);

        if let Some(parent) = manifest_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                PiCloudError::RegistryError {
                    reason: format!("Failed to create manifest dir: {e}"),
                }
            })?;
        }

        tokio::fs::write(&manifest_path, &data).await.map_err(|e| {
            PiCloudError::RegistryError {
                reason: format!("Failed to write manifest: {e}"),
            }
        })?;

        tokio::fs::write(&ct_path, content_type.as_bytes())
            .await
            .map_err(|e| PiCloudError::RegistryError {
                reason: format!("Failed to write content-type: {e}"),
            })?;

        // Also store by digest for direct digest pulls
        if !reference.starts_with("sha256:") {
            let digest_path = self.manifest_path(repository, &digest);
            let digest_ct_path = self.manifest_content_type_path(repository, &digest);
            tokio::fs::write(&digest_path, &data).await.ok();
            tokio::fs::write(&digest_ct_path, content_type.as_bytes())
                .await
                .ok();
        }

        // Store the manifest content as a blob too (for GC tracking)
        self.blob_store.put(&digest, &data).await.ok();

        debug!(
            repository = %repository,
            reference = %reference,
            digest = %digest,
            size = data.len(),
            "Manifest stored"
        );

        Ok(digest)
    }

    async fn get_manifest(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<(String, Vec<u8>)> {
        let manifest_path = self.manifest_path(repository, reference);
        let ct_path = self.manifest_content_type_path(repository, reference);

        let data = tokio::fs::read(&manifest_path).await.map_err(|_| {
            PiCloudError::ImageNotFound {
                repository: repository.to_string(),
                reference: reference.to_string(),
            }
        })?;

        let content_type = tokio::fs::read_to_string(&ct_path)
            .await
            .unwrap_or_else(|_| "application/vnd.oci.image.manifest.v1+json".to_string());

        Ok((content_type, data))
    }

    async fn delete_manifest(&self, repository: &str, reference: &str) -> Result<()> {
        let manifest_path = self.manifest_path(repository, reference);
        let ct_path = self.manifest_content_type_path(repository, reference);

        tokio::fs::remove_file(&manifest_path).await.ok();
        tokio::fs::remove_file(&ct_path).await.ok();
        Ok(())
    }

    async fn list_tags(&self, repository: &str) -> Result<Vec<String>> {
        let manifests_dir = self.repositories_dir().join(repository).join("manifests");

        if !manifests_dir.exists() {
            return Ok(Vec::new());
        }

        let mut tags = Vec::new();
        let dir = std::fs::read_dir(&manifests_dir).map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to read manifests dir: {e}"),
        })?;

        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip content-type metadata files and digest references
            if !name.ends_with(".content-type") && !name.starts_with("sha256_") {
                tags.push(name);
            }
        }

        tags.sort();
        Ok(tags)
    }

    async fn list_repositories(&self) -> Result<Vec<String>> {
        let repos_dir = self.repositories_dir();
        let mut repos = Vec::new();

        if !repos_dir.exists() {
            return Ok(repos);
        }

        let products = std::fs::read_dir(&repos_dir).map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to read repositories: {e}"),
        })?;

        for product_entry in products.flatten() {
            if !product_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let product = product_entry.file_name().to_string_lossy().to_string();
            let names = std::fs::read_dir(product_entry.path());
            for name_entry in names.into_iter().flatten().flatten() {
                if name_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = name_entry.file_name().to_string_lossy().to_string();
                    repos.push(format!("{product}/{name}"));
                }
            }
        }

        repos.sort();
        Ok(repos)
    }

    async fn start_blob_upload(&self, _repository: &str) -> Result<String> {
        let uuid = uuid::Uuid::new_v4().to_string();
        let upload_path = self.uploads_dir().join(&uuid);
        tokio::fs::write(&upload_path, b"").await.map_err(|e| {
            PiCloudError::RegistryError {
                reason: format!("Failed to create upload session: {e}"),
            }
        })?;
        debug!(uuid = %uuid, "Blob upload session started");
        Ok(uuid)
    }

    async fn garbage_collect(&self) -> Result<GarbageCollectionResult> {
        GarbageCollector::collect(&self.repositories_dir(), &self.blob_store).await
    }

    async fn stats(&self) -> Result<RegistryStats> {
        let repos = self.list_repositories().await?;
        let digests = self.blob_store.list_digests().await?;
        let total_size = self.blob_store.total_size().await.unwrap_or(0);

        Ok(RegistryStats {
            total_blobs: digests.len() as u64,
            total_repositories: repos.len() as u64,
            storage_used_bytes: total_size,
        })
    }
}

/// Sanitize a reference for use as a filename.
/// Tags are stored as-is. Digest references have `:` replaced with `_`.
fn sanitize_reference(reference: &str) -> String {
    reference.replace(':', "_")
}

/// Hex-encode a byte slice.
fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("picloud-reg-test-{}", uuid::Uuid::new_v4()));
        dir
    }

    #[tokio::test]
    async fn create_registry_and_store_manifest() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let manifest = br#"{"schemaVersion":2,"config":{"digest":"sha256:abc"},"layers":[]}"#;
        let digest = reg
            .put_manifest("photo-app/api", "1.0.0", "application/vnd.oci.image.manifest.v1+json", manifest.to_vec())
            .await
            .unwrap();

        assert!(digest.starts_with("sha256:"));

        let (ct, data) = reg.get_manifest("photo-app/api", "1.0.0").await.unwrap();
        assert_eq!(ct, "application/vnd.oci.image.manifest.v1+json");
        assert_eq!(data, manifest);
    }

    #[tokio::test]
    async fn list_tags() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let manifest = br#"{"schemaVersion":2}"#;
        reg.put_manifest("app/svc", "1.0.0", "application/json", manifest.to_vec())
            .await
            .unwrap();
        reg.put_manifest("app/svc", "2.0.0", "application/json", manifest.to_vec())
            .await
            .unwrap();

        let tags = reg.list_tags("app/svc").await.unwrap();
        assert_eq!(tags, vec!["1.0.0", "2.0.0"]);
    }

    #[tokio::test]
    async fn list_repositories() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let manifest = br#"{"schemaVersion":2}"#;
        reg.put_manifest("photo-app/api", "v1", "application/json", manifest.to_vec())
            .await
            .unwrap();
        reg.put_manifest("photo-app/worker", "v1", "application/json", manifest.to_vec())
            .await
            .unwrap();

        let repos = reg.list_repositories().await.unwrap();
        assert_eq!(repos, vec!["photo-app/api", "photo-app/worker"]);
    }

    #[tokio::test]
    async fn blob_round_trip() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let data = b"layer data here";
        let digest = format!(
            "sha256:{}",
            hex_encode(Sha256::digest(data))
        );

        reg.put_blob(&digest, data.to_vec()).await.unwrap();
        assert!(reg.blob_exists(&digest).await.unwrap());

        let retrieved = reg.get_blob(&digest).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn stats_reflect_stored_data() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let data = b"stats test blob";
        let digest = format!("sha256:{}", hex_encode(Sha256::digest(data)));
        reg.put_blob(&digest, data.to_vec()).await.unwrap();

        let stats = reg.stats().await.unwrap();
        assert_eq!(stats.total_blobs, 1);
    }

    #[tokio::test]
    async fn get_nonexistent_manifest_returns_error() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let result = reg.get_manifest("no-such/repo", "v1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn start_upload_returns_uuid() {
        let root = test_root();
        let reg = LocalRegistryBackend::new(&root).unwrap();

        let uuid = reg.start_blob_upload("app/svc").await.unwrap();
        assert!(!uuid.is_empty());
    }
}
