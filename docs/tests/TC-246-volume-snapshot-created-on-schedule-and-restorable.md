---
id: TC-246
title: Volume snapshot created on schedule and restorable to new volume
type: scenario
status: passing
validates:
  features:
  - FT-033
  adrs:
  - ADR-047
phase: 2
runner: cargo-test
runner-args: tc246_volume_snapshot_created_on_schedule_and_restorable_to_new_volume
last-run: 2026-04-18T14:09:30.707212670+00:00
last-run-duration: 0.6s
---

## Description

Verifies that the snapshot scheduler creates snapshots according to the
configured `SnapshotSchedule` (Hourly/Daily/Weekly) and that a snapshot
can be restored to a **new** volume (not just the original).

### Steps

1. Allocate a mounted volume and write known data.
2. Configure an Hourly snapshot schedule with retention (keep 3 daily).
3. Run the scheduler — first run should create a snapshot immediately.
4. Run the scheduler again within the hour — no snapshot should be created.
5. Simulate 1 hour elapsed — scheduler should create another snapshot.
6. Verify snapshots are listed.
7. Restore the first snapshot to a different volume IRI.
8. Verify the new volume contains the original data.
9. Create enough snapshots to exceed the retention limit.
10. Enforce retention — oldest snapshots are pruned to the configured limit.
11. Verify a disabled config produces no snapshots.