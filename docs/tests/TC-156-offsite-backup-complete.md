---
id: TC-156
title: offsite_backup_complete
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: picloud-test
runner-args: "offsite-backup-complete"
---

declare a volume with S3 offsite backup. Trigger a backup. Assert `BackupCompleted` event, backup metadata in RDF graph, and backup object present in the configured S3 bucket.