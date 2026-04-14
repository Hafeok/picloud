/// FT-034 Integration Tests — Offsite Backup
///
/// Covers TC-247, TC-304.
/// Tests S3-compatible offsite backup with client-side encryption and
/// incremental content-addressed deduplication.

use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_storage::backup::{
    BackupEncryption, InMemoryBackupTarget, OffsiteBackupManager, chunk_and_hash,
};

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

/// A fixed 256-bit test key (not used in production).
fn test_key() -> [u8; 32] {
    [0xABu8; 32]
}

// ============================================================================
// TC-247 — offsite_backup_uploads_encrypted_incremental_snapshot_to_s3_endpoint
// ============================================================================
/// Create a snapshot, run an offsite backup with encryption enabled.
/// Verify:
///   1. Encrypted chunks are uploaded to the backup target.
///   2. The uploaded data is NOT plaintext (client-side encryption).
///   3. A second backup of the same data deduplicates — zero new uploads.
///   4. A second backup with partially changed data only uploads the diff.
#[tokio::test]
async fn tc247_offsite_backup_uploads_encrypted_incremental_snapshot_to_s3_endpoint() {
    let ib = iri_builder();
    let volume_iri = ib.resource("photo-app", "volumes", "media");

    let target = InMemoryBackupTarget::new();
    let key = test_key();
    let manager = OffsiteBackupManager::new(target, key, "picloud/test-cluster".to_string())
        .with_chunk_size(64); // small chunks for testing

    // --- First backup ---
    let snapshot_data = b"Hello, PiCloud! This is snapshot data that is long enough to span multiple chunks for dedup testing.";
    let result = manager
        .backup_snapshot(&volume_iri, "2025-04-01T10-00-00Z", snapshot_data)
        .await
        .expect("first backup should succeed");

    // All chunks were new — nothing deduplicated
    assert!(result.total_chunks > 0, "should produce at least one chunk");
    assert_eq!(result.dedup_chunks, 0, "first backup has nothing to dedup");
    assert_eq!(result.uploaded_chunks, result.total_chunks);
    assert!(result.uploaded_bytes > 0);

    // --- Verify encryption: raw stored data must NOT match plaintext ---
    let keys = manager.target().keys().await;
    let chunk_keys: Vec<_> = keys.iter().filter(|k| k.contains("/chunks/")).collect();
    assert!(!chunk_keys.is_empty(), "chunks should be stored");

    for ck in &chunk_keys {
        let stored = manager.target().get(ck).await.unwrap();
        // Stored data includes 12-byte nonce prefix + ciphertext + 16-byte tag
        // It must NOT equal any plaintext chunk
        let chunks = chunk_and_hash(snapshot_data, 64);
        for chunk in &chunks {
            assert_ne!(
                stored, chunk.data,
                "stored chunk must not be plaintext — encryption failed"
            );
        }
    }

    // --- Second backup of identical data — full deduplication ---
    let result2 = manager
        .backup_snapshot(&volume_iri, "2025-04-02T10-00-00Z", snapshot_data)
        .await
        .expect("second backup should succeed");

    assert_eq!(
        result2.dedup_chunks, result2.total_chunks,
        "identical data should be fully deduplicated"
    );
    assert_eq!(result2.uploaded_chunks, 0, "no new chunks to upload");

    // --- Third backup with partially changed data — incremental ---
    let mut changed_data = snapshot_data.to_vec();
    // Modify only the first chunk (first 64 bytes)
    for b in changed_data.iter_mut().take(10) {
        *b = b'X';
    }
    let result3 = manager
        .backup_snapshot(&volume_iri, "2025-04-03T10-00-00Z", &changed_data)
        .await
        .expect("incremental backup should succeed");

    assert!(
        result3.uploaded_chunks < result3.total_chunks,
        "only changed chunks should be uploaded (got {} uploaded out of {} total)",
        result3.uploaded_chunks,
        result3.total_chunks
    );
    assert!(
        result3.dedup_chunks > 0,
        "unchanged chunks should be deduplicated"
    );

    // --- Restore and verify round-trip ---
    let restored = manager
        .restore_snapshot(&volume_iri, "2025-04-03T10-00-00Z")
        .await
        .expect("restore should succeed");
    assert_eq!(restored, changed_data, "restored data must match original");
}

// ============================================================================
// TC-304 — offsite_backup_exit_backup_uploaded_encrypted_to_s3_endpoint
// ============================================================================
/// Exit criterion: given a volume with snapshot data, after running the
/// offsite backup manager the data is stored encrypted on the S3-compatible
/// target and can be restored to its original form.
///
/// This is the gate check — it confirms the end-to-end contract:
///   input plaintext → encrypt → upload → download → decrypt → original plaintext
#[tokio::test]
async fn tc304_offsite_backup_exit_backup_uploaded_encrypted_to_s3_endpoint() {
    let ib = iri_builder();
    let volume_iri = ib.resource("critical-app", "volumes", "database");

    let target = InMemoryBackupTarget::new();
    let key = test_key();
    let manager = OffsiteBackupManager::new(target, key, "picloud/prod".to_string())
        .with_chunk_size(128);

    // Simulate a realistic snapshot — 1 KiB of data
    let snapshot_data: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();

    // --- Backup ---
    let result = manager
        .backup_snapshot(&volume_iri, "2025-06-15T08-30-00Z", &snapshot_data)
        .await
        .expect("backup must succeed");

    assert!(result.uploaded_chunks > 0, "data must be uploaded");
    assert!(result.uploaded_bytes > 0, "bytes must be uploaded");

    // --- Verify: stored data is encrypted (not plaintext) ---
    let all_keys = manager.target().keys().await;
    let chunk_keys: Vec<_> = all_keys.iter().filter(|k| k.contains("/chunks/")).collect();

    for ck in &chunk_keys {
        let encrypted_bytes = manager.target().get(ck).await.unwrap();
        // Must have nonce (12) + at least 1 byte ciphertext + tag (16)
        assert!(
            encrypted_bytes.len() >= 29,
            "encrypted chunk too short: {}",
            encrypted_bytes.len()
        );
    }

    // Manifest must also be encrypted
    let manifest_keys: Vec<_> = all_keys
        .iter()
        .filter(|k| k.contains("/manifests/"))
        .collect();
    assert_eq!(manifest_keys.len(), 1, "exactly one manifest expected");
    let manifest_bytes = manager.target().get(manifest_keys[0]).await.unwrap();
    // If it were plaintext JSON it would start with '['; encrypted data won't
    assert_ne!(
        manifest_bytes.first().copied(),
        Some(b'['),
        "manifest must be encrypted, not plaintext JSON"
    );

    // --- Restore and verify round-trip ---
    let restored = manager
        .restore_snapshot(&volume_iri, "2025-06-15T08-30-00Z")
        .await
        .expect("restore must succeed");

    assert_eq!(
        restored, snapshot_data,
        "restored data must exactly match original snapshot"
    );

    // --- Verify decryption with wrong key fails ---
    let wrong_key = [0xCDu8; 32];
    let bad_target = InMemoryBackupTarget::new();
    // Re-create manager with wrong key but share the same stored data
    // We'll test decrypt directly instead
    let encrypted = manager
        .target()
        .get(chunk_keys[0])
        .await
        .unwrap();
    let decrypt_result = BackupEncryption::decrypt(&wrong_key, &encrypted);
    assert!(
        decrypt_result.is_err(),
        "decryption with wrong key must fail"
    );

    // Verify correct key works
    let _ = bad_target; // unused — just proving wrong-key detection
    let decrypt_ok = BackupEncryption::decrypt(&key, &encrypted);
    assert!(decrypt_ok.is_ok(), "decryption with correct key must succeed");
}
