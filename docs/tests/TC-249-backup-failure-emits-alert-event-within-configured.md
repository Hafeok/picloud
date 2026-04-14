---
id: TC-249
title: Backup failure emits alert event within configured threshold
type: scenario
status: passing
runner: cargo-test
runner-args: "tc249_backup_failure_emits_alert_event_within_configured_threshold"
validates:
  features: [FT-036]
  adrs: [ADR-047]
phase: 2
last-run: 2026-04-14T08:30:40.623890577+00:00
---

## Description

Scenario: when a backup fails, the built-in BackupFailureAlertEvaluator
evaluates the BackupFailed event and produces an AlertFired action within
the configured threshold time.

### Verifications

1. A BackupFailed event is emitted when backup fails.
2. The BackupFailureAlertEvaluator produces an AlertAction::Fire for BackupFailed events.
3. The alert is produced within the configured threshold_secs.
4. The AlertFiredPayload contains the correct alert_type, severity, and resource_iri.
5. The message includes the backup failure reason.
6. Non-matching events (e.g. BackupCompleted) do not trigger alerts.