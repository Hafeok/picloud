---
id: TC-250
title: CLI volume snapshot create and volume restore round-trip
type: scenario
status: passing
runner: cargo-test
runner-args: "tc250_cli_volume_snapshot_create_and_volume_restore_round_trip"
validates:
  features: [FT-037]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-14T08:36:49.424932268+00:00
---

## Description

Verifies the full round-trip of the CLI volume snapshot and restore workflow:

1. Apply a product with a volume.
2. Create a snapshot via POST /api/volumes/snapshots (the endpoint the CLI `volume snapshot-create` subcommand calls).
3. Verify the SnapshotCreated event is emitted and persisted.
4. Restore the volume via POST /api/volumes/restore (the endpoint the CLI `volume restore` subcommand calls).
5. Verify the RestoreCompleted event is emitted.
6. List snapshots via GET /api/volumes/snapshots and verify the snapshot appears in the listing.