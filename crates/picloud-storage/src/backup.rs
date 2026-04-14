//! S3-compatible offsite backup client (ADR-047).
//!
//! Uploads encrypted, deduplicated, incremental backups to any
//! S3-compatible endpoint (Backblaze B2, Cloudflare R2, MinIO).
//!
//! Client-side encryption: data is encrypted with AES-256 before upload.
//! The encryption key comes from the platform secret store.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::ResourceIri;

/// Configuration for an S3-compatible backup target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BackupConfig {
    /// S3 endpoint URL (e.g., "https://s3.us-west-000.backblazeb2.com")
    pub endpoint: String,
    /// Bucket name
    pub bucket: String,
    /// Key prefix for all backups from this cluster
    pub prefix: String,
    /// Region (required by some providers)
    pub region: String,
}

/// An S3-compatible backup client.
pub struct S3BackupClient {
    config: S3BackupConfig,
    http: reqwest::Client,
}

impl S3BackupClient {
    /// Create a new backup client.
    pub fn new(config: S3BackupConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to create HTTP client for S3 backup");
        Self { config, http }
    }

    /// Upload a snapshot to the S3-compatible target.
    ///
    /// The data is assumed to already be encrypted by the caller
    /// (encryption happens at the storage layer, not in the HTTP client).
    pub async fn upload_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
        data: &[u8],
    ) -> Result<BackupRecord> {
        let key = format!(
            "{}/{}/{}",
            self.config.prefix,
            sanitize_iri_for_path(volume_iri.as_str()),
            snapshot_timestamp
        );

        let url = format!("{}/{}/{}", self.config.endpoint, self.config.bucket, key);

        info!(
            volume = %volume_iri,
            key = %key,
            size = data.len(),
            "Uploading backup to S3"
        );

        let response = self
            .http
            .put(&url)
            .body(data.to_vec())
            .header("Content-Type", "application/octet-stream")
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 upload failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(status = %status, body = %body, "S3 upload failed");
            return Err(PiCloudError::Internal(format!(
                "S3 upload failed with status {status}: {body}"
            )));
        }

        info!(volume = %volume_iri, key = %key, "Backup uploaded successfully");

        Ok(BackupRecord {
            volume_iri: volume_iri.clone(),
            s3_key: key,
            size_bytes: data.len() as u64,
            uploaded_at: Utc::now(),
        })
    }

    /// List all backups for a volume.
    pub async fn list_backups(&self, volume_iri: &ResourceIri) -> Result<Vec<BackupRecord>> {
        let prefix = format!(
            "{}/{}",
            self.config.prefix,
            sanitize_iri_for_path(volume_iri.as_str())
        );

        // S3 ListObjects v2 — simplified implementation
        let url = format!(
            "{}/{}?list-type=2&prefix={}",
            self.config.endpoint, self.config.bucket, prefix
        );

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 list failed: {e}")))?;

        if !response.status().is_success() {
            warn!(volume = %volume_iri, "Failed to list S3 backups");
            return Ok(Vec::new());
        }

        let body = response
            .text()
            .await
            .map_err(|e| PiCloudError::Internal(format!("Failed to read S3 list response: {e}")))?;

        // Parse S3 ListBucketResult XML
        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct ListBucketResult {
            #[serde(rename = "Contents")]
            contents: Vec<S3Object>,
        }

        #[derive(serde::Deserialize)]
        struct S3Object {
            #[serde(rename = "Key")]
            key: String,
            #[serde(rename = "Size")]
            size: u64,
            #[serde(rename = "LastModified")]
            last_modified: String,
        }

        let result: ListBucketResult = quick_xml::de::from_str(&body)
            .map_err(|e| PiCloudError::Internal(format!("Failed to parse S3 XML: {e}")))?;

        let backups = result
            .contents
            .into_iter()
            .filter_map(|item| {
                let uploaded_at = chrono::DateTime::parse_from_rfc3339(&item.last_modified)
                    .ok()?
                    .with_timezone(&Utc);
                Some(BackupRecord {
                    volume_iri: volume_iri.clone(),
                    s3_key: item.key,
                    size_bytes: item.size,
                    uploaded_at,
                })
            })
            .collect();

        Ok(backups)
    }

    /// Download a backup from S3.
    pub async fn download_backup(
        &self,
        s3_key: &str,
    ) -> Result<Vec<u8>> {
        let url = format!("{}/{}/{}", self.config.endpoint, self.config.bucket, s3_key);

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 download failed: {e}")))?;

        if !response.status().is_success() {
            return Err(PiCloudError::Internal(format!(
                "S3 download failed with status {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| PiCloudError::Internal(format!("Failed to read S3 response: {e}")))?;

        Ok(bytes.to_vec())
    }

    /// Delete a backup from S3.
    pub async fn delete_backup(&self, s3_key: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.config.endpoint, self.config.bucket, s3_key);

        let response = self
            .http
            .delete(&url)
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 delete failed: {e}")))?;

        if !response.status().is_success() {
            warn!(key = %s3_key, status = %response.status(), "S3 delete failed");
        }

        Ok(())
    }
}

/// Record of a completed backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub volume_iri: ResourceIri,
    pub s3_key: String,
    pub size_bytes: u64,
    pub uploaded_at: DateTime<Utc>,
}

/// Sanitize an IRI for use as an S3 key path component.
fn sanitize_iri_for_path(iri: &str) -> String {
    iri.replace("https://", "")
        .replace("http://", "")
        .replace(':', "_")
}

// ---------------------------------------------------------------------------
// Backup target abstraction — allows testing without a real S3 endpoint
// ---------------------------------------------------------------------------

/// Abstraction over a backup upload/download target.
/// `S3BackupClient` implements this for production; tests use `InMemoryBackupTarget`.
#[async_trait::async_trait]
pub trait BackupTarget: Send + Sync {
    /// Upload data under the given key. Overwrites if already present.
    async fn upload(&self, key: &str, data: &[u8]) -> Result<()>;
    /// Check whether a key already exists.
    async fn exists(&self, key: &str) -> Result<bool>;
    /// List all keys under a prefix.
    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>>;
    /// Download a key's contents.
    async fn download(&self, key: &str) -> Result<Vec<u8>>;
}

#[async_trait::async_trait]
impl BackupTarget for S3BackupClient {
    async fn upload(&self, key: &str, data: &[u8]) -> Result<()> {
        let url = format!("{}/{}/{}", self.config.endpoint, self.config.bucket, key);
        let response = self
            .http
            .put(&url)
            .body(data.to_vec())
            .header("Content-Type", "application/octet-stream")
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 upload failed: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(PiCloudError::Internal(format!("S3 upload failed: {status}")));
        }
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let url = format!("{}/{}/{}", self.config.endpoint, self.config.bucket, key);
        let response = self
            .http
            .head(&url)
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 HEAD failed: {e}")))?;
        Ok(response.status().is_success())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let url = format!(
            "{}/{}?list-type=2&prefix={}",
            self.config.endpoint, self.config.bucket, prefix
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| PiCloudError::Internal(format!("S3 list failed: {e}")))?;
        if !response.status().is_success() {
            return Ok(Vec::new());
        }
        let body = response.text().await.unwrap_or_default();

        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct ListBucketResult {
            #[serde(rename = "Contents")]
            contents: Vec<S3Obj>,
        }
        #[derive(serde::Deserialize)]
        struct S3Obj {
            #[serde(rename = "Key")]
            key: String,
        }
        let result: ListBucketResult = quick_xml::de::from_str(&body).unwrap_or_default();
        Ok(result.contents.into_iter().map(|o| o.key).collect())
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        self.download_backup(key).await
    }
}

/// In-memory backup target for testing — stores uploaded blobs in a `HashMap`.
///
/// Optionally simulates failures when `failure_message` is set.
pub struct InMemoryBackupTarget {
    store: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<u8>>>>,
    /// When set, all uploads will fail with this error message.
    failure_message: Option<String>,
}

impl InMemoryBackupTarget {
    pub fn new() -> Self {
        Self {
            store: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            failure_message: None,
        }
    }

    /// Create a failing backup target that returns errors on upload (FT-036).
    pub fn with_failure(mut self, message: &str) -> Self {
        self.failure_message = Some(message.to_string());
        self
    }

    /// Return a snapshot of all stored keys.
    pub async fn keys(&self) -> Vec<String> {
        let guard = self.store.read().await;
        guard.keys().cloned().collect()
    }

    /// Read the raw bytes stored under a key.
    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let guard = self.store.read().await;
        guard.get(key).cloned()
    }
}

#[async_trait::async_trait]
impl BackupTarget for InMemoryBackupTarget {
    async fn upload(&self, key: &str, data: &[u8]) -> Result<()> {
        if let Some(ref msg) = self.failure_message {
            return Err(picloud_domain::error::PiCloudError::Storage(msg.clone()));
        }
        let mut guard = self.store.write().await;
        guard.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let guard = self.store.read().await;
        Ok(guard.contains_key(key))
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let guard = self.store.read().await;
        Ok(guard
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn download(&self, key: &str) -> Result<Vec<u8>> {
        let guard = self.store.read().await;
        guard.get(key).cloned().ok_or_else(|| PiCloudError::ResourceNotFound {
            iri: key.to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Client-side encryption — AES-256-GCM via ring
// ---------------------------------------------------------------------------

/// AES-256-GCM client-side encryption for backup data.
///
/// Each call to `encrypt` generates a random 12-byte nonce. The output format
/// is: `nonce (12 bytes) || ciphertext+tag`. This matches the pattern used
/// by `picloud-iam/src/secrets.rs`.
pub struct BackupEncryption;

impl BackupEncryption {
    /// Encrypt `plaintext` with a 256-bit key. Returns `nonce || ciphertext || tag`.
    pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};

        let unbound = UnboundKey::new(&AES_256_GCM, key)
            .map_err(|_| PiCloudError::Internal("AES key creation failed".into()))?;
        let less_safe = LessSafeKey::new(unbound);

        let nonce_bytes = Self::random_nonce();
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut in_out = plaintext.to_vec();
        less_safe
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| PiCloudError::Internal("AES-256-GCM encryption failed".into()))?;

        // Prepend nonce
        let mut output = Vec::with_capacity(12 + in_out.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&in_out);
        Ok(output)
    }

    /// Decrypt data produced by `encrypt`. Input is `nonce (12) || ciphertext || tag`.
    pub fn decrypt(key: &[u8; 32], ciphertext_with_nonce: &[u8]) -> Result<Vec<u8>> {
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};

        if ciphertext_with_nonce.len() < 12 {
            return Err(PiCloudError::Internal(
                "Ciphertext too short — missing nonce".into(),
            ));
        }

        let (nonce_bytes, ciphertext) = ciphertext_with_nonce.split_at(12);
        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(nonce_bytes);

        let unbound = UnboundKey::new(&AES_256_GCM, key)
            .map_err(|_| PiCloudError::Internal("AES key creation failed".into()))?;
        let less_safe = LessSafeKey::new(unbound);
        let nonce = Nonce::assume_unique_for_key(nonce_arr);

        let mut in_out = ciphertext.to_vec();
        let plaintext = less_safe
            .open_in_place(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| PiCloudError::Internal("AES-256-GCM decryption failed".into()))?;

        Ok(plaintext.to_vec())
    }

    fn random_nonce() -> [u8; 12] {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut nonce = [0u8; 12];
        rng.fill(&mut nonce);
        nonce
    }
}

// ---------------------------------------------------------------------------
// Content-addressed chunk for incremental deduplication
// ---------------------------------------------------------------------------

/// A content-addressed chunk of snapshot data.
#[derive(Debug, Clone)]
pub struct ContentChunk {
    /// SHA-256 hex digest of the chunk's plaintext content.
    pub hash: String,
    /// Raw (unencrypted) chunk data.
    pub data: Vec<u8>,
}

/// Split raw snapshot data into fixed-size chunks and compute their SHA-256
/// content hashes. Duplicate chunks (same hash) are naturally deduplicated
/// when uploading — only chunks not already present on the target are sent.
pub fn chunk_and_hash(data: &[u8], chunk_size: usize) -> Vec<ContentChunk> {
    use sha2::{Digest, Sha256};

    data.chunks(chunk_size)
        .map(|chunk| {
            let mut hasher = Sha256::new();
            hasher.update(chunk);
            let hash = format!("{:x}", hasher.finalize());
            ContentChunk {
                hash,
                data: chunk.to_vec(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// OffsiteBackupManager — orchestrates snapshot → encrypt → dedup → upload
// ---------------------------------------------------------------------------

/// Default chunk size for incremental deduplication (256 KiB).
pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;

/// Orchestrates offsite backups with client-side encryption and incremental
/// content-addressed deduplication.
///
/// Flow:
/// 1. Read snapshot data from the local snapshot path.
/// 2. Split into fixed-size chunks and compute SHA-256 hashes.
/// 3. Check which chunks already exist on the backup target (dedup).
/// 4. Encrypt new chunks with AES-256-GCM.
/// 5. Upload encrypted chunks.
/// 6. Upload a manifest listing the ordered chunk hashes for this snapshot.
pub struct OffsiteBackupManager<T: BackupTarget> {
    target: T,
    encryption_key: [u8; 32],
    prefix: String,
    chunk_size: usize,
}

/// Result of a single backup operation.
#[derive(Debug, Clone)]
pub struct OffsiteBackupResult {
    /// S3 key prefix where the manifest was stored.
    pub manifest_key: String,
    /// Total chunks in the snapshot.
    pub total_chunks: usize,
    /// Chunks that were already present (deduplicated — not re-uploaded).
    pub dedup_chunks: usize,
    /// Chunks uploaded in this run.
    pub uploaded_chunks: usize,
    /// Total encrypted bytes uploaded.
    pub uploaded_bytes: u64,
}

impl<T: BackupTarget> OffsiteBackupManager<T> {
    /// Create a new manager.
    ///
    /// - `target`: where to upload encrypted chunks (S3, in-memory, etc.)
    /// - `encryption_key`: 256-bit AES key for client-side encryption
    /// - `prefix`: key prefix for all objects (e.g. `"picloud/cluster-1"`)
    pub fn new(target: T, encryption_key: [u8; 32], prefix: String) -> Self {
        Self {
            target,
            encryption_key,
            prefix,
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }

    /// Override the chunk size (useful for tests).
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = chunk_size;
        self
    }

    /// Run an incremental, encrypted, deduplicated backup of snapshot data.
    ///
    /// `volume_iri`: identifies the volume (used in key paths).
    /// `snapshot_timestamp`: unique identifier for this snapshot.
    /// `snapshot_data`: raw bytes of the snapshot.
    pub async fn backup_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
        snapshot_data: &[u8],
    ) -> Result<OffsiteBackupResult> {
        let volume_path = sanitize_iri_for_path(volume_iri.as_str());

        // 1. Chunk & hash
        let chunks = chunk_and_hash(snapshot_data, self.chunk_size);
        let total_chunks = chunks.len();

        // 2. Deduplicate — only upload chunks not already on the target
        let mut dedup_chunks = 0usize;
        let mut uploaded_chunks = 0usize;
        let mut uploaded_bytes = 0u64;

        let chunk_prefix = format!("{}/{}/chunks", self.prefix, volume_path);

        for chunk in &chunks {
            let chunk_key = format!("{}/{}", chunk_prefix, chunk.hash);

            if self.target.exists(&chunk_key).await? {
                dedup_chunks += 1;
                continue;
            }

            // 3. Encrypt
            let encrypted = BackupEncryption::encrypt(&self.encryption_key, &chunk.data)?;
            uploaded_bytes += encrypted.len() as u64;

            // 4. Upload
            self.target.upload(&chunk_key, &encrypted).await?;
            uploaded_chunks += 1;
        }

        // 5. Write manifest — an ordered list of chunk hashes
        let manifest: Vec<String> = chunks.iter().map(|c| c.hash.clone()).collect();
        let manifest_json = serde_json::to_vec(&manifest)
            .map_err(|e| PiCloudError::Internal(format!("manifest serialization: {e}")))?;

        // Encrypt the manifest too
        let encrypted_manifest =
            BackupEncryption::encrypt(&self.encryption_key, &manifest_json)?;

        let manifest_key = format!(
            "{}/{}/manifests/{}",
            self.prefix, volume_path, snapshot_timestamp
        );
        self.target
            .upload(&manifest_key, &encrypted_manifest)
            .await?;

        info!(
            volume = %volume_iri,
            snapshot = snapshot_timestamp,
            total_chunks,
            dedup_chunks,
            uploaded_chunks,
            uploaded_bytes,
            "Offsite backup completed"
        );

        Ok(OffsiteBackupResult {
            manifest_key,
            total_chunks,
            dedup_chunks,
            uploaded_chunks,
            uploaded_bytes,
        })
    }

    /// Restore a snapshot from the offsite backup.
    ///
    /// Downloads the manifest, then downloads and decrypts each chunk in order.
    pub async fn restore_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
    ) -> Result<Vec<u8>> {
        let volume_path = sanitize_iri_for_path(volume_iri.as_str());
        let manifest_key = format!(
            "{}/{}/manifests/{}",
            self.prefix, volume_path, snapshot_timestamp
        );

        // Download and decrypt manifest
        let encrypted_manifest = self.target.download(&manifest_key).await?;
        let manifest_json =
            BackupEncryption::decrypt(&self.encryption_key, &encrypted_manifest)?;
        let chunk_hashes: Vec<String> = serde_json::from_slice(&manifest_json)
            .map_err(|e| PiCloudError::Internal(format!("manifest parse: {e}")))?;

        // Download and decrypt each chunk in order
        let chunk_prefix = format!("{}/{}/chunks", self.prefix, volume_path);
        let mut output = Vec::new();

        for hash in &chunk_hashes {
            let chunk_key = format!("{}/{}", chunk_prefix, hash);
            let encrypted_chunk = self.target.download(&chunk_key).await?;
            let plaintext = BackupEncryption::decrypt(&self.encryption_key, &encrypted_chunk)?;
            output.extend_from_slice(&plaintext);
        }

        Ok(output)
    }

    /// Return a reference to the underlying backup target.
    pub fn target(&self) -> &T {
        &self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_iri_strips_protocol() {
        let result = sanitize_iri_for_path("https://picloud.local/products/app/volumes/data");
        assert_eq!(result, "picloud.local/products/app/volumes/data");
    }

    #[test]
    fn parse_s3_list_bucket_result() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Contents>
    <Key>picloud/cluster-1/picloud.local/products/app/volumes/data/2025-04-01T10:00:00Z</Key>
    <Size>1048576</Size>
    <LastModified>2025-04-01T10:00:00+00:00</LastModified>
  </Contents>
  <Contents>
    <Key>picloud/cluster-1/picloud.local/products/app/volumes/data/2025-04-02T10:00:00Z</Key>
    <Size>2097152</Size>
    <LastModified>2025-04-02T10:00:00+00:00</LastModified>
  </Contents>
</ListBucketResult>"#;

        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct ListBucketResult {
            #[serde(rename = "Contents")]
            contents: Vec<S3Obj>,
        }

        #[derive(serde::Deserialize)]
        struct S3Obj {
            #[serde(rename = "Key")]
            key: String,
            #[serde(rename = "Size")]
            size: u64,
            #[serde(rename = "LastModified")]
            last_modified: String,
        }

        let result: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(result.contents.len(), 2);
        assert_eq!(result.contents[0].size, 1048576);
        assert!(result.contents[1].key.contains("2025-04-02"));
    }

    #[test]
    fn parse_empty_s3_list() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult></ListBucketResult>"#;

        #[derive(serde::Deserialize, Default)]
        #[serde(default)]
        struct ListBucketResult {
            #[serde(rename = "Contents")]
            contents: Vec<S3Obj>,
        }

        #[derive(serde::Deserialize)]
        struct S3Obj {
            #[serde(rename = "Key")]
            key: String,
        }

        let result: ListBucketResult = quick_xml::de::from_str(xml).unwrap();
        assert!(result.contents.is_empty());
    }

    #[test]
    fn s3_backup_config_serde() {
        let config = S3BackupConfig {
            endpoint: "https://s3.backblaze.com".to_string(),
            bucket: "my-backups".to_string(),
            prefix: "picloud/cluster-1".to_string(),
            region: "us-west-000".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: S3BackupConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bucket, "my-backups");
    }
}
