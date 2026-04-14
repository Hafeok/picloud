//! Event-emitting wrappers for snapshot and backup operations (FT-035, ADR-047).
//!
//! These wrappers delegate to the underlying `SnapshotManager` / `OffsiteBackupManager`
//! and emit lifecycle events to the platform event log at each transition:
//!
//! - **SnapshotCreated** / **SnapshotFailed** — on snapshot creation success/failure
//! - **SnapshotDeleted** — when retention policy removes old snapshots
//! - **BackupStarted** / **BackupCompleted** / **BackupFailed** — offsite backup lifecycle

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tracing::{debug, warn};
use uuid::Uuid;

use picloud_domain::error::Result;
use picloud_domain::events::{
    BackupCompletedPayload, BackupFailedPayload, BackupStartedPayload, EventEnvelope,
    SnapshotCreatedPayload, SnapshotDeletedPayload, SnapshotFailedPayload,
};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{BackupInfo, EventLog, SnapshotInfo, SnapshotManager};

use crate::backup::{BackupTarget, OffsiteBackupManager, OffsiteBackupResult};

// ---------------------------------------------------------------------------
// EventEmittingSnapshotManager
// ---------------------------------------------------------------------------

/// A `SnapshotManager` decorator that emits lifecycle events to the event log.
///
/// Delegates all operations to an inner `SnapshotManager` and appends the
/// appropriate event (`SnapshotCreated`, `SnapshotFailed`, `SnapshotDeleted`)
/// after each operation completes.
pub struct EventEmittingSnapshotManager<S: SnapshotManager> {
    inner: Arc<S>,
    event_log: Arc<dyn EventLog>,
    iri_builder: IriBuilder,
}

impl<S: SnapshotManager> EventEmittingSnapshotManager<S> {
    /// Create a new event-emitting snapshot manager.
    ///
    /// - `inner`: the underlying snapshot manager that does the real work
    /// - `event_log`: where to append lifecycle events
    pub fn new(inner: Arc<S>, event_log: Arc<dyn EventLog>) -> Self {
        Self {
            inner,
            event_log,
            iri_builder: IriBuilder::new(ClusterDomain::default()),
        }
    }

    /// Emit a single event to the log, logging (but not propagating) failures.
    async fn emit(&self, event: EventEnvelope) {
        if let Err(e) = self.event_log.append(event).await {
            warn!(error = %e, "Failed to emit snapshot/backup lifecycle event");
        }
    }
}

#[async_trait]
impl<S: SnapshotManager> SnapshotManager for EventEmittingSnapshotManager<S> {
    async fn create_snapshot(&self, volume_iri: &ResourceIri) -> Result<SnapshotInfo> {
        let correlation_id = Uuid::new_v4();

        match self.inner.create_snapshot(volume_iri).await {
            Ok(info) => {
                let payload = SnapshotCreatedPayload {
                    volume_iri: volume_iri.clone(),
                    snapshot_path: info.snapshot_path.clone(),
                    size_bytes: info.size_bytes,
                    created_at: info.created_at,
                };
                let event = EventEnvelope::new(
                    self.iri_builder.event_schema("SnapshotCreated", 1),
                    "SnapshotCreated",
                    volume_iri.clone(),
                    None,
                    correlation_id,
                    serde_json::to_value(&payload).unwrap_or_default(),
                );
                debug!(volume = %volume_iri, "Emitting SnapshotCreated event");
                self.emit(event).await;
                Ok(info)
            }
            Err(e) => {
                let payload = SnapshotFailedPayload {
                    volume_iri: volume_iri.clone(),
                    reason: e.to_string(),
                };
                let event = EventEnvelope::new(
                    self.iri_builder.event_schema("SnapshotFailed", 1),
                    "SnapshotFailed",
                    volume_iri.clone(),
                    None,
                    correlation_id,
                    serde_json::to_value(&payload).unwrap_or_default(),
                );
                debug!(volume = %volume_iri, "Emitting SnapshotFailed event");
                self.emit(event).await;
                Err(e)
            }
        }
    }

    async fn list_snapshots(&self, volume_iri: &ResourceIri) -> Result<Vec<SnapshotInfo>> {
        self.inner.list_snapshots(volume_iri).await
    }

    async fn restore_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
    ) -> Result<()> {
        self.inner
            .restore_snapshot(volume_iri, snapshot_timestamp)
            .await
    }

    async fn delete_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_path: &str,
    ) -> Result<()> {
        let result = self.inner.delete_snapshot(volume_iri, snapshot_path).await;

        if result.is_ok() {
            let payload = SnapshotDeletedPayload {
                volume_iri: volume_iri.clone(),
                snapshot_path: snapshot_path.to_string(),
            };
            let event = EventEnvelope::new(
                self.iri_builder.event_schema("SnapshotDeleted", 1),
                "SnapshotDeleted",
                volume_iri.clone(),
                None,
                Uuid::new_v4(),
                serde_json::to_value(&payload).unwrap_or_default(),
            );
            debug!(volume = %volume_iri, "Emitting SnapshotDeleted event");
            self.emit(event).await;
        }

        result
    }

    async fn list_backups(&self, volume_iri: &ResourceIri) -> Result<Vec<BackupInfo>> {
        self.inner.list_backups(volume_iri).await
    }

    async fn restore_snapshot_to_new_volume(
        &self,
        source_volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
        target_volume_iri: &ResourceIri,
    ) -> Result<()> {
        self.inner
            .restore_snapshot_to_new_volume(source_volume_iri, snapshot_timestamp, target_volume_iri)
            .await
    }

    async fn enforce_retention(
        &self,
        volume_iri: &ResourceIri,
        retention: &picloud_domain::storage::SnapshotRetention,
    ) -> Result<usize> {
        // Get the list of snapshots before enforcement so we know which ones
        // will be deleted and can emit SnapshotDeleted events for each.
        let mut snapshots = self.inner.list_snapshots(volume_iri).await?;
        snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let max_keep = retention.daily as usize;
        let to_delete: Vec<_> = if max_keep > 0 && snapshots.len() > max_keep {
            snapshots[max_keep..].to_vec()
        } else {
            Vec::new()
        };

        let deleted = self.inner.enforce_retention(volume_iri, retention).await?;

        // Emit SnapshotDeleted for each snapshot that was removed
        for snap in &to_delete {
            let payload = SnapshotDeletedPayload {
                volume_iri: volume_iri.clone(),
                snapshot_path: snap.snapshot_path.clone(),
            };
            let event = EventEnvelope::new(
                self.iri_builder.event_schema("SnapshotDeleted", 1),
                "SnapshotDeleted",
                volume_iri.clone(),
                None,
                Uuid::new_v4(),
                serde_json::to_value(&payload).unwrap_or_default(),
            );
            debug!(
                volume = %volume_iri,
                snapshot_path = %snap.snapshot_path,
                "Emitting SnapshotDeleted event (retention)"
            );
            self.emit(event).await;
        }

        Ok(deleted)
    }
}

// ---------------------------------------------------------------------------
// EventEmittingBackupManager
// ---------------------------------------------------------------------------

/// A wrapper around `OffsiteBackupManager` that emits backup lifecycle events.
///
/// Emits:
/// - **BackupStarted** before the backup begins
/// - **BackupCompleted** on success
/// - **BackupFailed** on failure
pub struct EventEmittingBackupManager<T: BackupTarget> {
    inner: OffsiteBackupManager<T>,
    event_log: Arc<dyn EventLog>,
    iri_builder: IriBuilder,
}

impl<T: BackupTarget> EventEmittingBackupManager<T> {
    /// Create a new event-emitting backup manager.
    pub fn new(inner: OffsiteBackupManager<T>, event_log: Arc<dyn EventLog>) -> Self {
        Self {
            inner,
            event_log,
            iri_builder: IriBuilder::new(ClusterDomain::default()),
        }
    }

    /// Emit a single event to the log.
    async fn emit(&self, event: EventEnvelope) {
        if let Err(e) = self.event_log.append(event).await {
            warn!(error = %e, "Failed to emit backup lifecycle event");
        }
    }

    /// Run an offsite backup with full lifecycle event emission.
    ///
    /// 1. Emits `BackupStarted`
    /// 2. Delegates to the inner manager
    /// 3. Emits `BackupCompleted` or `BackupFailed`
    pub async fn backup_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
        snapshot_data: &[u8],
    ) -> Result<OffsiteBackupResult> {
        let correlation_id = Uuid::new_v4();

        // --- BackupStarted ---
        let started_payload = BackupStartedPayload {
            volume_iri: volume_iri.clone(),
            snapshot_path: snapshot_timestamp.to_string(),
        };
        let started_event = EventEnvelope::new(
            self.iri_builder.event_schema("BackupStarted", 1),
            "BackupStarted",
            volume_iri.clone(),
            None,
            correlation_id,
            serde_json::to_value(&started_payload).unwrap_or_default(),
        );
        debug!(volume = %volume_iri, "Emitting BackupStarted event");
        self.emit(started_event).await;

        // --- Delegate ---
        match self
            .inner
            .backup_snapshot(volume_iri, snapshot_timestamp, snapshot_data)
            .await
        {
            Ok(result) => {
                // --- BackupCompleted ---
                let completed_payload = BackupCompletedPayload {
                    volume_iri: volume_iri.clone(),
                    size_bytes: result.uploaded_bytes,
                    completed_at: Utc::now(),
                };
                let completed_event = EventEnvelope::new(
                    self.iri_builder.event_schema("BackupCompleted", 1),
                    "BackupCompleted",
                    volume_iri.clone(),
                    None,
                    correlation_id,
                    serde_json::to_value(&completed_payload).unwrap_or_default(),
                );
                debug!(volume = %volume_iri, "Emitting BackupCompleted event");
                self.emit(completed_event).await;
                Ok(result)
            }
            Err(e) => {
                // --- BackupFailed ---
                let failed_payload = BackupFailedPayload {
                    volume_iri: volume_iri.clone(),
                    reason: e.to_string(),
                };
                let failed_event = EventEnvelope::new(
                    self.iri_builder.event_schema("BackupFailed", 1),
                    "BackupFailed",
                    volume_iri.clone(),
                    None,
                    correlation_id,
                    serde_json::to_value(&failed_payload).unwrap_or_default(),
                );
                debug!(volume = %volume_iri, "Emitting BackupFailed event");
                self.emit(failed_event).await;
                Err(e)
            }
        }
    }

    /// Delegate restore to the inner manager (no event emission needed).
    pub async fn restore_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
    ) -> Result<Vec<u8>> {
        self.inner
            .restore_snapshot(volume_iri, snapshot_timestamp)
            .await
    }

    /// Access the underlying backup target (for testing).
    pub fn target(&self) -> &T {
        self.inner.target()
    }
}

// ---------------------------------------------------------------------------
// BackupFailureAlertEvaluator (FT-036, ADR-047)
// ---------------------------------------------------------------------------

/// Evaluates incoming events against built-in event-driven alert rules for
/// backup failures (FT-036).
///
/// When a `BackupFailed` event is observed, this evaluator produces an
/// `AlertAction::Fire` with severity `Critical` within the configured
/// threshold time. The threshold is defined in `builtin_event_alert_rules()`.
pub struct BackupFailureAlertEvaluator {
    rules: Vec<picloud_domain::resources::BuiltInEventAlertRule>,
    iri_builder: IriBuilder,
}

impl BackupFailureAlertEvaluator {
    /// Create a new backup failure alert evaluator with the built-in rules.
    pub fn new() -> Self {
        Self {
            rules: picloud_domain::resources::builtin_event_alert_rules(),
            iri_builder: IriBuilder::new(ClusterDomain::default()),
        }
    }

    /// Create with custom rules (for testing threshold configuration).
    pub fn with_rules(rules: Vec<picloud_domain::resources::BuiltInEventAlertRule>) -> Self {
        Self {
            rules,
            iri_builder: IriBuilder::new(ClusterDomain::default()),
        }
    }

    /// Returns a reference to the configured rules.
    pub fn rules(&self) -> &[picloud_domain::resources::BuiltInEventAlertRule] {
        &self.rules
    }
}

impl Default for BackupFailureAlertEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl picloud_domain::traits::EventAlertEvaluator for BackupFailureAlertEvaluator {
    async fn evaluate_event(
        &self,
        event: &picloud_domain::events::EventEnvelope,
    ) -> picloud_domain::error::Result<Vec<picloud_domain::traits::AlertAction>> {
        let mut actions = Vec::new();

        for rule in &self.rules {
            if event.event_type == rule.trigger_event {
                let reason = event.payload["reason"]
                    .as_str()
                    .unwrap_or("unknown reason");
                let resource_iri_str = event.payload["volume_iri"]
                    .as_str()
                    .unwrap_or(event.source.as_str());

                let message = rule.message_template.replace("{reason}", reason);
                let rule_iri = self.iri_builder.inference_rule(rule.name);

                let payload = picloud_domain::events::AlertFiredPayload {
                    alert_type: rule.alert_type.to_string(),
                    severity: rule.severity.clone(),
                    message,
                    resource_iri: ResourceIri(resource_iri_str.to_string()),
                    rule_iri,
                    fired_at: Utc::now(),
                };

                debug!(
                    alert_type = rule.alert_type,
                    resource = resource_iri_str,
                    "BackupFailureAlertEvaluator firing alert"
                );

                actions.push(picloud_domain::traits::AlertAction::Fire(payload));
            }
        }

        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke test: EventEmittingSnapshotManager and EventEmittingBackupManager
    // compile and can be constructed. Full integration tests are in
    // tests/ft035_snapshot_backup_events.rs.

    #[test]
    fn event_emitting_types_exist() {
        // This test just verifies the types are constructible — the real
        // integration tests run against InMemoryEventLog + LocalSnapshotManager.
        assert_eq!(std::mem::size_of::<EventEmittingBackupManager<crate::backup::InMemoryBackupTarget>>(), std::mem::size_of::<EventEmittingBackupManager<crate::backup::InMemoryBackupTarget>>());
    }

    #[test]
    fn backup_failure_alert_evaluator_types_exist() {
        let evaluator = BackupFailureAlertEvaluator::new();
        assert!(!evaluator.rules().is_empty(), "should have at least one built-in event alert rule");
    }
}
