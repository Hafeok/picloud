---
id: TC-157
title: backup_failure_alert
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: picloud-test
runner-args: "backup-failure-alert"
---

configure an invalid S3 endpoint. Trigger a backup. Assert `BackupFailed` event and `AlertFired` (type: `BackupFailed`, severity: `critical`) within 30 seconds.