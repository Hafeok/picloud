---
id: TC-157
title: backup_failure_alert
type: scenario
status: passing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
---

configure an invalid S3 endpoint. Trigger a backup. Assert `BackupFailed` event and `AlertFired` (type: `BackupFailed`, severity: `critical`) within 30 seconds.