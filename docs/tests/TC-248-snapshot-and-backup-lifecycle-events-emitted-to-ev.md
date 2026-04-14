---
id: TC-248
title: Snapshot and backup lifecycle events emitted to event log
type: scenario
status: passing
runner: cargo-test
runner-args: "tc248_snapshot_and_backup_lifecycle_events_emitted_to_event_log"
validates:
  features: [FT-035]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-14T08:23:06.605604339+00:00
---

## Description

Scenario: when snapshot and backup operations execute through the
`EventEmittingSnapshotManager` and `EventEmittingBackupManager` wrappers, the
correct lifecycle events are emitted to the platform event log.

Verifies:
1. **SnapshotCreated** is emitted on successful snapshot creation with correct payload (volume_iri, snapshot_path, size_bytes, created_at).
2. **SnapshotFailed** is emitted when snapshot creation fails (e.g. volume does not exist) with a reason.
3. **SnapshotDeleted** events are emitted when retention policy removes old snapshots (one per deleted snapshot).
4. **BackupStarted** is emitted before an offsite backup begins.
5. **BackupCompleted** is emitted after a successful backup with size_bytes and completed_at.
6. **Correlation IDs** link BackupStarted and BackupCompleted events within the same operation.