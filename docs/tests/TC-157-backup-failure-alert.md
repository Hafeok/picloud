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
runner: scripts/run-tc.sh
runner-args: "backup-failure-alert"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

configure an invalid S3 endpoint. Trigger a backup. Assert `BackupFailed` event and `AlertFired` (type: `BackupFailed`, severity: `critical`) within 30 seconds.