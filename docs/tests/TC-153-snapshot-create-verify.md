---
id: TC-153
title: snapshot_create_verify
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: scripts/run-tc.sh
runner-args: "snapshot-create-verify"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

declare a volume with daily snapshots and a NAS target. Trigger a snapshot (via `picloud volume snapshot now`). Assert `SnapshotCreated` event emitted and the snapshot file is present on the NAS at the expected path.