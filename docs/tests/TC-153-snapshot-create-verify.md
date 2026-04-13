---
id: TC-153
title: snapshot_create_verify
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: picloud-test
runner-args: "snapshot-create-verify"
---

declare a volume with daily snapshots and a NAS target. Trigger a snapshot (via `picloud volume snapshot now`). Assert `SnapshotCreated` event emitted and the snapshot file is present on the NAS at the expected path.