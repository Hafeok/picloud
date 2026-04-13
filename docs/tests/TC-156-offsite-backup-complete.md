---
id: TC-156
title: offsite_backup_complete
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: scripts/run-tc.sh
runner-args: "offsite-backup-complete"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

declare a volume with S3 offsite backup. Trigger a backup. Assert `BackupCompleted` event, backup metadata in RDF graph, and backup object present in the configured S3 bucket.