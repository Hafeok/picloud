//! Content-addressed blob storage for OCI images (ADR-054).
//!
//! Blobs are stored at `{root}/blobs/sha256/{digest}`.
//! Layer deduplication is automatic — same content = same digest = same file.
//! Digest is verified on write to prevent content corruption.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs;
use tracing::{debug, warn};

use picloud_domain::error::{PiCloudError, Result};

/// Content-addressed blob storage on the local filesystem.
pub struct BlobStore {
    /// Root directory for blob storage (e.g. `/var/lib/picloud/registry/blobs`)
    root: PathBuf,
}

impl BlobStore {
    /// Create a new blob store at the given root path.
    pub fn new(root: &Path) -> Result<Self> {
        let blobs_dir = root.join("sha256");
        std::fs::create_dir_all(&blobs_dir).map_err(|e| {
            PiCloudError::RegistryError {
                reason: format!("Failed to create blob store at {}: {e}", blobs_dir.display()),
            }
        })?;
        Ok(Self { root: root.to_path_buf() })
    }

    /// Path for a blob by digest (without the `sha256:` prefix).
    fn blob_path(&self, digest: &str) -> PathBuf {
        let hash = digest.strip_prefix("sha256:").unwrap_or(digest);
        self.root.join("sha256").join(hash)
    }

    /// Store a blob. Verifies the digest matches the content.
    pub async fn put(&self, digest: &str, data: &[u8]) -> Result<()> {
        let expected_hash = digest.strip_prefix("sha256:").unwrap_or(digest);

        // Verify content digest
        let actual_hash = hex::encode(Sha256::digest(data));
        if actual_hash != expected_hash {
            return Err(PiCloudError::RegistryError {
                reason: format!(
                    "Digest mismatch: expected sha256:{expected_hash}, got sha256:{actual_hash}"
                ),
            });
        }

        let path = self.blob_path(digest);
        if path.exists() {
            debug!(digest = %digest, "Blob already exists (dedup)");
            return Ok(());
        }

        fs::write(&path, data).await.map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to write blob {digest}: {e}"),
        })?;

        debug!(digest = %digest, size = data.len(), "Blob stored");
        Ok(())
    }

    /// Retrieve a blob by digest.
    pub async fn get(&self, digest: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(digest);
        fs::read(&path).await.map_err(|_| PiCloudError::BlobNotFound {
            digest: digest.to_string(),
        })
    }

    /// Check if a blob exists.
    pub async fn exists(&self, digest: &str) -> bool {
        self.blob_path(digest).exists()
    }

    /// Delete a blob.
    pub async fn delete(&self, digest: &str) -> Result<()> {
        let path = self.blob_path(digest);
        if path.exists() {
            fs::remove_file(&path).await.map_err(|e| PiCloudError::RegistryError {
                reason: format!("Failed to delete blob {digest}: {e}"),
            })?;
        }
        Ok(())
    }

    /// List all blob digests in the store.
    pub async fn list_digests(&self) -> Result<Vec<String>> {
        let sha_dir = self.root.join("sha256");
        let mut digests = Vec::new();

        let mut entries = fs::read_dir(&sha_dir).await.map_err(|e| {
            PiCloudError::RegistryError {
                reason: format!("Failed to read blob directory: {e}"),
            }
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            PiCloudError::RegistryError {
                reason: format!("Failed to read blob entry: {e}"),
            }
        })? {
            if let Some(name) = entry.file_name().to_str() {
                digests.push(format!("sha256:{name}"));
            }
        }

        Ok(digests)
    }

    /// Total size of all blobs in bytes.
    pub async fn total_size(&self) -> Result<u64> {
        let sha_dir = self.root.join("sha256");
        let mut total = 0u64;

        let mut entries = fs::read_dir(&sha_dir).await.unwrap_or_else(|_| {
            warn!("Cannot read blob directory for size calculation");
            // Return a dummy that yields no entries
            unreachable!()
        });

        // Fallback: use sync read_dir for robustness
        let dir = std::fs::read_dir(&sha_dir).map_err(|e| PiCloudError::RegistryError {
            reason: format!("Failed to read blob directory: {e}"),
        })?;

        for entry in dir.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }

        // Suppress unused variable warning
        drop(entries);

        Ok(total)
    }
}

/// Hex encoding utility (avoiding extra dependency).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get_blob() {
        let dir = tempdir();
        let store = BlobStore::new(&dir).unwrap();

        let data = b"hello world";
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(data)));

        store.put(&digest, data).await.unwrap();
        assert!(store.exists(&digest).await);

        let retrieved = store.get(&digest).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn put_wrong_digest_fails() {
        let dir = tempdir();
        let store = BlobStore::new(&dir).unwrap();

        let result = store.put("sha256:0000000000000000000000000000000000000000000000000000000000000000", b"hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn dedup_same_content() {
        let dir = tempdir();
        let store = BlobStore::new(&dir).unwrap();

        let data = b"dedup test";
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(data)));

        store.put(&digest, data).await.unwrap();
        store.put(&digest, data).await.unwrap(); // should not error (dedup)

        let digests = store.list_digests().await.unwrap();
        assert_eq!(digests.len(), 1);
    }

    #[tokio::test]
    async fn delete_blob() {
        let dir = tempdir();
        let store = BlobStore::new(&dir).unwrap();

        let data = b"to delete";
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(data)));

        store.put(&digest, data).await.unwrap();
        assert!(store.exists(&digest).await);

        store.delete(&digest).await.unwrap();
        assert!(!store.exists(&digest).await);
    }

    #[tokio::test]
    async fn get_nonexistent_blob_returns_error() {
        let dir = tempdir();
        let store = BlobStore::new(&dir).unwrap();

        let result = store.get("sha256:nonexistent").await;
        assert!(result.is_err());
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("picloud-blob-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
