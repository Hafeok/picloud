---
id: TC-303
title: Volume snapshots exit — snapshot created, listed, and restored
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc303_volume_snapshots_exit_snapshot_created_listed_and_restored"
validates:
  features: [FT-033]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-18T11:08:58.593422020+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria for volume snapshots: validates that the core snapshot
lifecycle (create, list, restore, delete) works end-to-end.

### Exit criteria verified

1. A snapshot can be **created** from a mounted volume with data.
2. Created snapshots are **listed** with correct metadata (path, size, volume IRI).
3. After mutating volume data, **restoring** from a snapshot recovers original contents.
4. Multiple snapshots are listed in newest-first order.
5. A snapshot can be restored to a **new** (different) volume.
6. A snapshot can be **deleted**, and the listing reflects the removal.