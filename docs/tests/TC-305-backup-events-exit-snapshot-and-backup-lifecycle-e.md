---
id: TC-305
title: Backup events exit — snapshot and backup lifecycle events emitted
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc305_backup_events_exit_snapshot_and_backup_lifecycle_events_emitted"
validates:
  features: [FT-035]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-14T08:23:06.605604339+00:00
---

## Description

Exit criterion: the full set of snapshot and backup lifecycle events is emitted
correctly, including the failure path.

Gate checks:
1. Successful snapshot emits **SnapshotCreated** (not SnapshotFailed).
2. Failed snapshot emits **SnapshotFailed** (not SnapshotCreated).
3. Snapshot deletion emits **SnapshotDeleted** with the correct snapshot_path.
4. Successful backup emits **BackupStarted** then **BackupCompleted** in order.
5. All events carry correct **schema IRIs** (containing event type name and /v1 version), correct **source IRIs** (matching the volume), and well-formed payloads.
6. **Correlation IDs** link related events within an operation (BackupStarted and BackupCompleted share the same correlation_id).