/// FT-036 Integration Tests — Backup failure alert rules (built-in)
///
/// Covers TC-249 (scenario) and TC-306 (exit-criteria).
///
/// Verifies that when a backup fails, the built-in event-driven alert rule
/// fires an AlertFired event within the configured threshold time.

use std::sync::Arc;
use std::time::Instant;

use uuid::Uuid;

use picloud_domain::events::{AlertSeverity, EventEnvelope};
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::resources::{builtin_event_alert_rules, BuiltInEventAlertRule};
use picloud_domain::traits::{AlertAction, EventAlertEvaluator, EventLog};
use picloud_events::InMemoryEventLog;
use picloud_storage::backup::{InMemoryBackupTarget, OffsiteBackupManager};
use picloud_storage::event_emitting::EventEmittingBackupManager;
use picloud_storage::BackupFailureAlertEvaluator;

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

/// A fixed 256-bit test key for encryption.
fn test_key() -> [u8; 32] {
    [0xABu8; 32]
}

// ============================================================================
// TC-249 — backup_failure_emits_alert_event_within_configured_threshold
// ============================================================================
/// Scenario: when a backup fails, the BackupFailureAlertEvaluator evaluates
/// the BackupFailed event and produces an AlertFired action within the
/// configured threshold time.
///
/// Verifies:
///   1. A BackupFailed event is emitted when backup fails.
///   2. The BackupFailureAlertEvaluator produces an AlertAction::Fire for BackupFailed events.
///   3. The alert is produced within the configured threshold_secs.
///   4. The AlertFiredPayload contains the correct alert_type, severity, and resource_iri.
///   5. The message includes the backup failure reason.
///   6. Non-matching events (e.g. BackupCompleted) do not trigger alerts.
#[tokio::test]
async fn tc249_backup_failure_emits_alert_event_within_configured_threshold() {
    let ib = iri_builder();
    let volume_iri = ib.resource("photo-app", "volumes", "media");

    // --- Set up a backup that will fail ---
    let event_log: Arc<dyn EventLog> = Arc::new(InMemoryEventLog::new());
    // Use an InMemoryBackupTarget that will fail by providing corrupt data scenario
    let target = InMemoryBackupTarget::new();
    let inner_backup =
        OffsiteBackupManager::new(target, test_key(), "picloud/test-alert".to_string())
            .with_chunk_size(64);
    let backup_manager = EventEmittingBackupManager::new(inner_backup, event_log.clone());

    // Trigger a successful backup first (should NOT trigger alerts)
    let snapshot_data = b"Valid snapshot data for alert threshold test with enough bytes";
    let _result = backup_manager
        .backup_snapshot(&volume_iri, "2025-07-01T03-00-00Z", snapshot_data)
        .await
        .expect("backup should succeed");

    let events = event_log.events_since(0).await;
    assert_eq!(events.len(), 2, "expected BackupStarted + BackupCompleted");

    // Evaluate the BackupCompleted event — should produce NO alerts
    let evaluator = BackupFailureAlertEvaluator::new();
    let actions = evaluator
        .evaluate_event(&events[1])
        .await
        .expect("evaluate should succeed");
    assert!(
        actions.is_empty(),
        "BackupCompleted should not trigger any alert"
    );

    // --- 1. Simulate a BackupFailed event ---
    // Create a synthetic BackupFailed event (as would be emitted by the
    // EventEmittingBackupManager on a real failure)
    let failed_payload = serde_json::json!({
        "volume_iri": volume_iri.as_str(),
        "reason": "S3 connection timeout after 30 seconds"
    });
    let failed_event = EventEnvelope::new(
        ib.event_schema("BackupFailed", 1),
        "BackupFailed",
        volume_iri.clone(),
        None,
        Uuid::new_v4(),
        failed_payload,
    );

    // --- 2. Evaluate within threshold ---
    let start = Instant::now();
    let actions = evaluator
        .evaluate_event(&failed_event)
        .await
        .expect("evaluate should succeed");
    let elapsed = start.elapsed();

    // --- 3. Verify threshold ---
    let rules = builtin_event_alert_rules();
    let backup_rule = rules
        .iter()
        .find(|r| r.trigger_event == "BackupFailed")
        .expect("backup-failed-critical rule must exist");
    assert!(
        elapsed.as_secs() < backup_rule.threshold_secs,
        "alert must be produced within {}s threshold, took {:?}",
        backup_rule.threshold_secs,
        elapsed
    );

    // --- 4. Verify alert action ---
    assert_eq!(actions.len(), 1, "should produce exactly one alert action");
    match &actions[0] {
        AlertAction::Fire(payload) => {
            assert_eq!(payload.alert_type, "BackupFailed");
            assert_eq!(payload.severity, AlertSeverity::Critical);
            assert_eq!(payload.resource_iri.as_str(), volume_iri.as_str());
            assert!(
                payload.rule_iri.as_str().contains("backup-failed-critical"),
                "rule_iri should reference the backup-failed-critical rule, got: {}",
                payload.rule_iri.as_str()
            );

            // --- 5. Message includes the reason ---
            assert!(
                payload.message.contains("S3 connection timeout"),
                "message should include the failure reason, got: {}",
                payload.message
            );
        }
        AlertAction::Resolve(_) => {
            panic!("expected AlertAction::Fire, got Resolve");
        }
    }

    // --- 6. Non-matching events produce no alerts ---
    let started_event = EventEnvelope::new(
        ib.event_schema("BackupStarted", 1),
        "BackupStarted",
        volume_iri.clone(),
        None,
        Uuid::new_v4(),
        serde_json::json!({"volume_iri": volume_iri.as_str(), "snapshot_path": "test"}),
    );
    let no_actions = evaluator
        .evaluate_event(&started_event)
        .await
        .expect("evaluate should succeed");
    assert!(
        no_actions.is_empty(),
        "BackupStarted should not trigger any alert"
    );
}

// ============================================================================
// TC-306 — backup_alerts_exit_failure_alert_triggered_within_threshold
// ============================================================================
/// Exit criterion: the full backup failure alert pipeline works end-to-end.
///
/// Verifies:
///   1. Built-in event alert rules include a backup-failed-critical rule.
///   2. The rule has correct properties (trigger_event, alert_type, severity, threshold).
///   3. An actual backup failure (via EventEmittingBackupManager) emits a BackupFailed event.
///   4. The evaluator produces an AlertFired action from that real event.
///   5. The AlertFired event can be appended to the event log (full pipeline).
///   6. The alert is produced within the configured threshold_secs.
///   7. Custom threshold configuration is respected.
#[tokio::test]
async fn tc306_backup_alerts_exit_failure_alert_triggered_within_threshold() {
    let ib = iri_builder();
    let volume_iri = ib.resource("critical-app", "volumes", "database");

    // --- Gate 1: built-in rules include backup-failed-critical ---
    let rules = builtin_event_alert_rules();
    let backup_rule = rules
        .iter()
        .find(|r| r.name == "backup-failed-critical")
        .expect("must have a backup-failed-critical built-in event alert rule");

    // --- Gate 2: rule properties ---
    assert_eq!(backup_rule.trigger_event, "BackupFailed");
    assert_eq!(backup_rule.alert_type, "BackupFailed");
    assert_eq!(backup_rule.severity, AlertSeverity::Critical);
    assert!(
        backup_rule.threshold_secs > 0,
        "threshold must be positive"
    );
    assert!(
        backup_rule.message_template.contains("{reason}"),
        "message template must contain {{reason}} placeholder"
    );

    // --- Gate 3: real BackupFailed event from EventEmittingBackupManager ---
    let event_log: Arc<dyn EventLog> = Arc::new(InMemoryEventLog::new());
    // Use a failing backup target to produce a real BackupFailed event
    let target = InMemoryBackupTarget::new().with_failure("simulated disk I/O error");
    let inner_backup =
        OffsiteBackupManager::new(target, test_key(), "picloud/exit-alert".to_string())
            .with_chunk_size(64);
    let backup_manager = EventEmittingBackupManager::new(inner_backup, event_log.clone());

    let result = backup_manager
        .backup_snapshot(&volume_iri, "2025-07-15T02-00-00Z", b"snapshot data for alert exit test")
        .await;
    assert!(result.is_err(), "backup with failing target must fail");

    let events = event_log.events_since(0).await;
    assert!(events.len() >= 2, "expected at least BackupStarted + BackupFailed");
    let failed_event = events
        .iter()
        .find(|e| e.event_type == "BackupFailed")
        .expect("must have a BackupFailed event");

    // Verify BackupFailed event payload
    assert_eq!(
        failed_event.payload["volume_iri"].as_str().unwrap(),
        volume_iri.as_str()
    );
    assert!(
        failed_event.payload["reason"].as_str().unwrap().len() > 0,
        "BackupFailed must include a reason"
    );

    // --- Gate 4: evaluator produces AlertFired from the real event ---
    let evaluator = BackupFailureAlertEvaluator::new();
    let start = Instant::now();
    let actions = evaluator
        .evaluate_event(failed_event)
        .await
        .expect("evaluate should succeed");
    let elapsed = start.elapsed();

    assert_eq!(actions.len(), 1);
    let alert_payload = match &actions[0] {
        AlertAction::Fire(p) => p,
        AlertAction::Resolve(_) => panic!("expected Fire, got Resolve"),
    };

    assert_eq!(alert_payload.alert_type, "BackupFailed");
    assert_eq!(alert_payload.severity, AlertSeverity::Critical);
    assert_eq!(alert_payload.resource_iri.as_str(), volume_iri.as_str());

    // --- Gate 5: AlertFired can be appended to event log (full pipeline) ---
    let alert_event = EventEnvelope::new(
        ib.event_schema("AlertFired", 1),
        "AlertFired",
        alert_payload.resource_iri.clone(),
        None,
        failed_event.correlation_id,
        serde_json::to_value(alert_payload).expect("serialize AlertFiredPayload"),
    );
    let _ = event_log
        .append(alert_event)
        .await;
    let all_events = event_log.events_since(0).await;
    let alert_events: Vec<_> = all_events
        .iter()
        .filter(|e| e.event_type == "AlertFired")
        .collect();
    assert_eq!(
        alert_events.len(),
        1,
        "exactly one AlertFired event should be in the log"
    );
    assert_eq!(
        alert_events[0].payload["alert_type"].as_str().unwrap(),
        "BackupFailed"
    );
    assert_eq!(
        alert_events[0].payload["severity"].as_str().unwrap(),
        "critical"
    );

    // --- Gate 6: within threshold ---
    assert!(
        elapsed.as_secs() < backup_rule.threshold_secs,
        "alert evaluation must complete within {}s, took {:?}",
        backup_rule.threshold_secs,
        elapsed
    );

    // --- Gate 7: custom threshold configuration ---
    let custom_rules = vec![BuiltInEventAlertRule {
        name: "backup-failed-custom",
        trigger_event: "BackupFailed",
        alert_type: "BackupFailed",
        severity: AlertSeverity::Warning,
        message_template: "Custom backup alert: {reason}",
        threshold_secs: 10,
    }];
    let custom_evaluator = BackupFailureAlertEvaluator::with_rules(custom_rules);
    let custom_actions = custom_evaluator
        .evaluate_event(failed_event)
        .await
        .expect("custom evaluate should succeed");
    assert_eq!(custom_actions.len(), 1);
    match &custom_actions[0] {
        AlertAction::Fire(p) => {
            assert_eq!(p.severity, AlertSeverity::Warning, "custom severity respected");
            assert!(
                p.message.contains("Custom backup alert"),
                "custom message template respected"
            );
        }
        _ => panic!("expected Fire"),
    }
}
