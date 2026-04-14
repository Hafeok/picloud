---
id: TC-306
title: Backup alerts exit — failure alert triggered within threshold
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc306_backup_alerts_exit_failure_alert_triggered_within_threshold"
validates:
  features: [FT-036]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-14T08:30:40.623890577+00:00
---

## Description

Exit criterion: the full backup failure alert pipeline works end-to-end.

### Exit criteria verified

1. Built-in event alert rules include a `backup-failed-critical` rule.
2. The rule has correct properties (trigger_event, alert_type, severity, threshold).
3. An actual backup failure (via EventEmittingBackupManager) emits a BackupFailed event.
4. The evaluator produces an AlertFired action from that real event.
5. The AlertFired event can be appended to the event log (full pipeline).
6. The alert is produced within the configured threshold_secs.
7. Custom threshold configuration is respected.