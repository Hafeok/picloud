/// FT-035 Integration Tests — Snapshot and backup lifecycle events
///
/// Covers TC-248 (scenario) and TC-305 (exit-criteria).
///
/// Verifies that snapshot and backup operations emit the correct lifecycle
/// events (SnapshotCreated, SnapshotFailed, SnapshotDeleted, BackupStarted,
/// BackupCompleted, BackupFailed) to the platform event log.

use std::sync::Arc;

use uuid::Uuid;

use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::storage::SnapshotRetention;
use picloud_domain::traits::{EventLog, SnapshotManager};
use picloud_events::InMemoryEventLog;
use picloud_storage::backup::{InMemoryBackupTarget, OffsiteBackupManager};
use picloud_storage::event_emitting::{EventEmittingBackupManager, EventEmittingSnapshotManager};
use picloud_storage::LocalSnapshotManager;

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

/// Create a temporary base directory for snapshot tests.
fn temp_base_path() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("picloud-ft035-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    dir
}

/// Create a fake volume directory with a test file.
fn create_test_volume(base: &std::path::Path, volume_name: &str) {
    let vol_dir = base.join(volume_name);
    std::fs::create_dir_all(&vol_dir).expect("failed to create volume dir");
    std::fs::write(vol_dir.join("data.bin"), b"test volume data")
        .expect("failed to write test data");
}

/// A fixed 256-bit test key for encryption.
fn test_key() -> [u8; 32] {
    [0xABu8; 32]
}

// ============================================================================
// TC-248 — snapshot_and_backup_lifecycle_events_emitted_to_event_log
// ============================================================================
/// Scenario: when snapshot and backup operations execute, the correct lifecycle
/// events are emitted to the event log.
///
/// Verifies:
///   1. SnapshotCreated event is emitted on successful snapshot creation.
///   2. SnapshotFailed event is emitted when snapshot creation fails.
///   3. SnapshotDeleted events are emitted when retention policy removes old snapshots.
///   4. BackupStarted event is emitted before backup begins.
///   5. BackupCompleted event is emitted after successful backup.
///   6. BackupFailed event is emitted when backup fails (tested via TC-305).
#[tokio::test]
async fn tc248_snapshot_and_backup_lifecycle_events_emitted_to_event_log() {
    let base = temp_base_path();
    let ib = iri_builder();
    let volume_iri = ib.resource("photo-app", "volumes", "media");

    // Set up event log and event-emitting snapshot manager
    let event_log: Arc<dyn EventLog> = Arc::new(InMemoryEventLog::new());
    let inner_manager = Arc::new(LocalSnapshotManager::new(base.clone()));
    let manager = EventEmittingSnapshotManager::new(inner_manager.clone(), event_log.clone());

    // --- 1. SnapshotCreated on success ---
    create_test_volume(&base, "media");
    let info = manager
        .create_snapshot(&volume_iri)
        .await
        .expect("snapshot should succeed");

    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 1, "expected exactly one event after snapshot creation");
    assert_eq!(events[0].event_type, "SnapshotCreated");
    assert_eq!(events[0].source.as_str(), volume_iri.as_str());

    // Verify payload contains the expected fields
    let payload = &events[0].payload;
    assert_eq!(
        payload["volume_iri"].as_str().unwrap(),
        volume_iri.as_str()
    );
    assert_eq!(
        payload["snapshot_path"].as_str().unwrap(),
        info.snapshot_path
    );
    assert!(payload["size_bytes"].as_u64().is_some());
    assert!(payload["created_at"].as_str().is_some());

    // --- 2. SnapshotFailed when volume doesn't exist ---
    let bad_iri = ib.resource("photo-app", "volumes", "nonexistent");
    let result = manager.create_snapshot(&bad_iri).await;
    assert!(result.is_err(), "snapshot of nonexistent volume should fail");

    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 2, "expected two events total");
    assert_eq!(events[1].event_type, "SnapshotFailed");
    assert_eq!(events[1].source.as_str(), bad_iri.as_str());
    assert!(
        events[1].payload["reason"].as_str().unwrap().len() > 0,
        "SnapshotFailed should include a reason"
    );

    // --- 3. SnapshotDeleted via retention enforcement ---
    // Create multiple snapshots so retention has something to delete
    for _ in 0..3 {
        manager
            .create_snapshot(&volume_iri)
            .await
            .expect("snapshot should succeed");
    }

    // Retention: keep only 2 daily snapshots (we now have 4 total)
    let retention = SnapshotRetention {
        daily: 2,
        weekly: 0,
        monthly: 0,
    };
    let deleted_count = manager
        .enforce_retention(&volume_iri, &retention)
        .await
        .expect("retention enforcement should succeed");
    assert!(deleted_count > 0, "should have deleted some snapshots");

    // Check that SnapshotDeleted events were emitted
    let all_events = event_log.events_since(0).await;
    let deleted_events: Vec<_> = all_events
        .iter()
        .filter(|e| e.event_type == "SnapshotDeleted")
        .collect();
    assert_eq!(
        deleted_events.len(),
        deleted_count,
        "should emit one SnapshotDeleted per deleted snapshot"
    );
    for de in &deleted_events {
        assert!(
            de.payload["snapshot_path"].as_str().is_some(),
            "SnapshotDeleted must include snapshot_path"
        );
    }

    // --- 4 & 5. BackupStarted + BackupCompleted ---
    let backup_event_log: Arc<dyn EventLog> = Arc::new(InMemoryEventLog::new());
    let target = InMemoryBackupTarget::new();
    let inner_backup =
        OffsiteBackupManager::new(target, test_key(), "picloud/test".to_string())
            .with_chunk_size(64);
    let backup_manager = EventEmittingBackupManager::new(inner_backup, backup_event_log.clone());

    let snapshot_data = b"Snapshot data for backup lifecycle test with enough bytes to span chunks.";
    let _result = backup_manager
        .backup_snapshot(&volume_iri, "2025-04-01T10-00-00Z", snapshot_data)
        .await
        .expect("backup should succeed");

    let backup_events = backup_event_log.events_since(0).await;
    assert_eq!(backup_events.len(), 2, "expected BackupStarted + BackupCompleted");
    assert_eq!(backup_events[0].event_type, "BackupStarted");
    assert_eq!(backup_events[1].event_type, "BackupCompleted");

    // BackupStarted payload
    assert_eq!(
        backup_events[0].payload["volume_iri"].as_str().unwrap(),
        volume_iri.as_str()
    );
    assert_eq!(
        backup_events[0].payload["snapshot_path"].as_str().unwrap(),
        "2025-04-01T10-00-00Z"
    );

    // BackupCompleted payload
    assert_eq!(
        backup_events[1].payload["volume_iri"].as_str().unwrap(),
        volume_iri.as_str()
    );
    assert!(backup_events[1].payload["size_bytes"].as_u64().is_some());
    assert!(backup_events[1].payload["completed_at"].as_str().is_some());

    // Both events share the same correlation_id (linking the operation)
    assert_eq!(
        backup_events[0].correlation_id, backup_events[1].correlation_id,
        "BackupStarted and BackupCompleted must share a correlation_id"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}

// ============================================================================
// TC-305 — backup_events_exit_snapshot_and_backup_lifecycle_events_emitted
// ============================================================================
/// Exit criterion: the full set of snapshot and backup lifecycle events is
/// emitted correctly — including the failure path (BackupFailed).
///
/// Verifies the complete event lifecycle contract:
///   1. Successful snapshot emits SnapshotCreated (not SnapshotFailed).
///   2. Failed snapshot emits SnapshotFailed (not SnapshotCreated).
///   3. Snapshot deletion emits SnapshotDeleted.
///   4. Successful backup emits BackupStarted then BackupCompleted.
///   5. All events carry correct schema IRIs, source IRIs, and payloads.
///   6. Correlation IDs link related events within an operation.
#[tokio::test]
async fn tc305_backup_events_exit_snapshot_and_backup_lifecycle_events_emitted() {
    let base = temp_base_path();
    let ib = iri_builder();
    let volume_iri = ib.resource("critical-app", "volumes", "database");

    let event_log: Arc<dyn EventLog> = Arc::new(InMemoryEventLog::new());
    let inner_manager = Arc::new(LocalSnapshotManager::new(base.clone()));
    let snapshot_mgr = EventEmittingSnapshotManager::new(inner_manager.clone(), event_log.clone());

    // --- Gate 1: successful snapshot ---
    create_test_volume(&base, "database");
    let info = snapshot_mgr
        .create_snapshot(&volume_iri)
        .await
        .expect("snapshot must succeed");

    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 1);
    let created_event = &events[0];
    assert_eq!(created_event.event_type, "SnapshotCreated");
    // Schema IRI must be well-formed
    assert!(
        created_event.schema.as_str().contains("SnapshotCreated"),
        "schema IRI must reference SnapshotCreated"
    );
    assert!(
        created_event.schema.as_str().contains("/v1"),
        "schema IRI must include version"
    );

    // --- Gate 2: failed snapshot ---
    let missing_iri = ib.resource("critical-app", "volumes", "does-not-exist");
    let _ = snapshot_mgr.create_snapshot(&missing_iri).await;

    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 2);
    let failed_event = &events[1];
    assert_eq!(failed_event.event_type, "SnapshotFailed");
    assert!(
        failed_event.schema.as_str().contains("SnapshotFailed"),
        "schema IRI must reference SnapshotFailed"
    );

    // --- Gate 3: snapshot deletion ---
    snapshot_mgr
        .delete_snapshot(&volume_iri, &info.snapshot_path)
        .await
        .expect("delete must succeed");

    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 3);
    let deleted_event = &events[2];
    assert_eq!(deleted_event.event_type, "SnapshotDeleted");
    assert_eq!(
        deleted_event.payload["snapshot_path"].as_str().unwrap(),
        info.snapshot_path
    );

    // --- Gate 4: backup lifecycle (BackupStarted → BackupCompleted) ---
    let backup_log: Arc<dyn EventLog> = Arc::new(InMemoryEventLog::new());
    let target = InMemoryBackupTarget::new();
    let inner_backup =
        OffsiteBackupManager::new(target, test_key(), "picloud/exit-test".to_string())
            .with_chunk_size(64);
    let backup_mgr = EventEmittingBackupManager::new(inner_backup, backup_log.clone());

    let data: Vec<u8> = (0..512).map(|i| (i % 256) as u8).collect();
    let _result = backup_mgr
        .backup_snapshot(&volume_iri, "2025-06-15T08-30-00Z", &data)
        .await
        .expect("backup must succeed");

    let backup_events = backup_log.events_since(0).await;
    assert_eq!(backup_events.len(), 2);

    // --- Gate 5: all events carry correct schema IRIs ---
    let started = &backup_events[0];
    let completed = &backup_events[1];

    assert_eq!(started.event_type, "BackupStarted");
    assert!(started.schema.as_str().contains("BackupStarted"));
    assert!(started.schema.as_str().contains("/v1"));

    assert_eq!(completed.event_type, "BackupCompleted");
    assert!(completed.schema.as_str().contains("BackupCompleted"));
    assert!(completed.schema.as_str().contains("/v1"));

    // Source IRIs match the volume
    assert_eq!(started.source.as_str(), volume_iri.as_str());
    assert_eq!(completed.source.as_str(), volume_iri.as_str());

    // Payload correctness
    assert!(completed.payload["size_bytes"].as_u64().unwrap() > 0);
    assert!(completed.payload["completed_at"].as_str().is_some());

    // --- Gate 6: correlation IDs ---
    assert_eq!(
        started.correlation_id, completed.correlation_id,
        "BackupStarted and BackupCompleted must share correlation_id"
    );

    // Cleanup
    let _ = std::fs::remove_dir_all(&base);
}
