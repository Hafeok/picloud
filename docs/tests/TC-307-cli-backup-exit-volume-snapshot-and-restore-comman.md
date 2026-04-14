---
id: TC-307
title: CLI backup exit — volume snapshot and restore commands work
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc307_cli_backup_exit_volume_snapshot_and_restore_commands_work"
validates:
  features: [FT-037]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-14T08:36:49.424932268+00:00
---

## Description

Exit-criteria test verifying that all CLI volume snapshot, backup, and restore commands are functional:

1. Snapshot creation endpoint accepts requests and emits events.
2. Snapshot listing endpoint returns well-formed results.
3. Volume restore endpoint accepts requests and emits events.
4. Backup endpoint accepts requests and emits BackupCompleted events.
5. The resolve_volume_iri helper correctly resolves volume paths to IRIs.