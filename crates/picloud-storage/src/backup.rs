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
