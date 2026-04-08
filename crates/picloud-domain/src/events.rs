/// Platform Event Types
///
/// Every state change in PiCloud is an event (ADR-004).
/// Every event carries a schema IRI for versioning (ADR-031).
/// Events are append-only and permanent.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::iri::ResourceIri;

/// The envelope that wraps every event in the platform log.
/// The schema field is the IRI of the JSON Schema / SHACL document
/// describing this event's payload structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: Uuid,
    /// IRI of the schema for this event — permanently dereferenceable
    pub schema: ResourceIri,
    /// Human-readable event type name
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    /// IRI of the resource that emitted this event
    pub source: ResourceIri,
    /// Product scope — None for platform-level events
    pub product: Option<String>,
    /// Correlation ID linking a command to its result events
    pub correlation_id: Uuid,
    /// Client-supplied idempotency key (ADR-015)
    pub idempotency_key: Option<String>,
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    pub fn new(
        schema: ResourceIri,
        event_type: impl Into<String>,
        source: ResourceIri,
        product: Option<String>,
        correlation_id: Uuid,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            schema,
            event_type: event_type.into(),
            timestamp: Utc::now(),
            source,
            product,
            correlation_id,
            idempotency_key: None,
            payload,
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

/// Platform-level event types
/// Each variant corresponds to a schema IRI at:
/// https://picloud.local/schemas/events/{TypeName}/v1
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum PlatformEvent {
    // --- Cluster events ---
    NodeJoined(NodeJoinedPayload),
    NodeLeft(NodeLeftPayload),
    LeaderElected(LeaderElectedPayload),

    // --- Resource lifecycle events ---
    ResourceDeclared(ResourceDeclaredPayload),
    ResourceProvisioning(ResourceProvisioningPayload),
    ResourceReady(ResourceReadyPayload),
    ResourceFailed(ResourceFailedPayload),
    ResourceDeleted(ResourceDeletedPayload),

    // --- IAM events ---
    IdentityCreated(IdentityCreatedPayload),
    IdentityDeleted(IdentityDeletedPayload),
    PasskeyRegistered(PasskeyRegisteredPayload),
    PasskeyRevoked(PasskeyRevokedPayload),
    TokenIssued(TokenIssuedPayload),

    // --- Product events ---
    ProductDeployed(ProductDeployedPayload),
    ProductUpgradeStarted(ProductUpgradeStartedPayload),
    ProductUpgradeCompleted(ProductUpgradeCompletedPayload),
    ProductUpgradeAborted(ProductUpgradeAbortedPayload),
    ProductDeleted(ProductDeletedPayload),

    // --- Tag events (ADR-036) ---
    TagAdded(TagAddedPayload),
    TagRemoved(TagRemovedPayload),

    // --- Metrics events (ADR-040) ---
    MetricRecorded(MetricRecordedPayload),

    // --- Configuration events (ADR-043) ---
    ConfigChanged(ConfigChangedPayload),

    // --- Feature flag events (ADR-044) ---
    FeatureFlagChanged(FeatureFlagChangedPayload),

    // --- Alert events (ADR-041) ---
    AlertFired(AlertFiredPayload),
    AlertResolved(AlertResolvedPayload),

    // --- Cluster identity events (ADR-042) ---
    ClusterInitialized(ClusterInitializedPayload),
    NodeJoinRejected(NodeJoinRejectedPayload),

    // --- Group events (ADR-037) ---
    GroupMembershipChanged(GroupMembershipChangedPayload),

    // --- Inference events (ADR-038) ---
    InferenceRuleEvaluated(InferenceRuleEvaluatedPayload),
    ReconciliationCompleted(ReconciliationCompletedPayload),

    // --- Replay events (ADR-035) ---
    ReplayStarted(ReplayStartedPayload),
    ReplayProgress(ReplayProgressPayload),
    ReplayCompleted(ReplayCompletedPayload),
    ReplayFailed(ReplayFailedPayload),

    // --- Telemetry events (ADR-045) ---
    TelemetryAggregated(TelemetryAggregatedPayload),

    // --- Node failure / workload rescheduling events ---
    NodeUnreachable(NodeUnreachablePayload),
    WorkloadRescheduled(WorkloadRescheduledPayload),
}

// --- Payload types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeJoinedPayload {
    pub node_id: Uuid,
    pub node_name: String,
    pub node_iri: ResourceIri,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLeftPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderElectedPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub term: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDeclaredPayload {
    pub resource_iri: ResourceIri,
    pub resource_type: String,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceProvisioningPayload {
    pub resource_iri: ResourceIri,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReadyPayload {
    pub resource_iri: ResourceIri,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceFailedPayload {
    pub resource_iri: ResourceIri,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceDeletedPayload {
    pub resource_iri: ResourceIri,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityCreatedPayload {
    pub identity_iri: ResourceIri,
    pub identity_type: IdentityType,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityType {
    Human,
    Workload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityDeletedPayload {
    pub identity_iri: ResourceIri,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyRegisteredPayload {
    pub identity_iri: ResourceIri,
    pub credential_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyRevokedPayload {
    pub identity_iri: ResourceIri,
    pub credential_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenIssuedPayload {
    pub identity_iri: ResourceIri,
    pub product: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDeployedPayload {
    pub product_iri: ResourceIri,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductUpgradeStartedPayload {
    pub product_iri: ResourceIri,
    pub from_version: String,
    pub to_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductUpgradeCompletedPayload {
    pub product_iri: ResourceIri,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductUpgradeAbortedPayload {
    pub product_iri: ResourceIri,
    pub version: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductDeletedPayload {
    pub product_iri: ResourceIri,
}

// --- Metrics payloads (ADR-040) ---

/// A single metric data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEntry {
    /// Metric name, e.g. "cpu_usage_percent", "memory_used_mb".
    pub name: String,
    /// Numeric value of the metric.
    pub value: f64,
    /// Unit of the metric, e.g. "percent", "mb", "gb", "celsius".
    pub unit: String,
}

/// Payload for hardware metrics recorded by the platform metrics agent.
/// Emitted every collection interval (default 15s) per node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRecordedPayload {
    pub node_iri: ResourceIri,
    pub metrics: Vec<MetricEntry>,
}

/// Payload for TagAdded event (ADR-036).
/// Emitted when a tag is added to a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagAddedPayload {
    pub resource_iri: ResourceIri,
    pub key: String,
    pub value: String,
}

/// Payload for TagRemoved event (ADR-036).
/// Emitted when a tag is removed from a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRemovedPayload {
    pub resource_iri: ResourceIri,
    pub key: String,
    pub value: String,
}

/// Payload for ConfigChanged event (ADR-043).
/// Emitted when a configuration entry is created, updated, or deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangedPayload {
    pub config_iri: ResourceIri,
    pub product: String,
    pub key: String,
    pub value: Option<String>,
    pub config_type: Option<String>,
    /// "set" or "deleted"
    pub action: String,
}

/// Payload for FeatureFlagChanged event (ADR-044).
/// Emitted when a feature flag is created, updated, or deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagChangedPayload {
    pub flag_iri: ResourceIri,
    pub product: String,
    pub flag_name: String,
    pub enabled: Option<bool>,
    pub version_expr: Option<String>,
    /// "set" or "deleted"
    pub action: String,
}

// --- Alert payloads (ADR-041) ---

/// Alert severity levels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertSeverity::Info => write!(f, "info"),
            AlertSeverity::Warning => write!(f, "warning"),
            AlertSeverity::Critical => write!(f, "critical"),
        }
    }
}

/// Payload for AlertFired event (ADR-041).
/// Emitted when a SPARQL CONSTRUCT rule produces a picloud:Alert triple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertFiredPayload {
    /// Type of alert, e.g. "HighCpuTemperature", "HighMemoryUsage"
    pub alert_type: String,
    /// Severity: info, warning, or critical
    pub severity: AlertSeverity,
    /// Human-readable message
    pub message: String,
    /// IRI of the resource the alert is about
    pub resource_iri: ResourceIri,
    /// IRI of the inference rule that fired this alert
    pub rule_iri: ResourceIri,
    /// When the alert fired
    pub fired_at: DateTime<Utc>,
}

/// Payload for AlertResolved event (ADR-041).
/// Emitted when the CONSTRUCT query no longer produces the alert triple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertResolvedPayload {
    /// Type of alert that was resolved
    pub alert_type: String,
    /// IRI of the resource that was alerting
    pub resource_iri: ResourceIri,
    /// IRI of the inference rule that resolved
    pub rule_iri: ResourceIri,
    /// When the alert was resolved
    pub resolved_at: DateTime<Utc>,
}

// --- Cluster identity payloads (ADR-042) ---

/// Payload for ClusterInitialized event (ADR-042).
/// Emitted once at cluster init — establishes immutable cluster identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInitializedPayload {
    /// Unique cluster identifier (UUID generated at init)
    pub cluster_id: Uuid,
    /// Human-readable cluster domain (e.g. "picloud.local")
    pub domain: String,
    /// SHA-256 fingerprint of the cluster CA certificate
    pub ca_fingerprint: String,
}

/// Payload for NodeJoinRejected event (ADR-042).
/// Emitted when a node attempts to join but fails validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeJoinRejectedPayload {
    /// The node ID that was rejected
    pub node_id: Uuid,
    /// The address the node attempted to join from
    pub address: String,
    /// Human-readable reason for rejection
    pub reason: String,
}

// --- Group payloads (ADR-037) ---

/// Payload for GroupMembershipChanged event (ADR-037).
/// Emitted when inference rules add or remove members from a group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMembershipChangedPayload {
    /// IRI of the group whose membership changed
    pub group_iri: ResourceIri,
    /// IRI of the user who was added or removed
    pub member_iri: ResourceIri,
    /// "added" or "removed"
    pub action: String,
}

// --- Inference payloads (ADR-038) ---

/// Payload for InferenceRuleEvaluated event (ADR-038).
/// Emitted after a rule is evaluated, summarising assertions and retractions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRuleEvaluatedPayload {
    /// IRI of the inference rule that was evaluated
    pub rule_iri: ResourceIri,
    /// Number of new triples asserted
    pub assertions: usize,
    /// Number of triples retracted
    pub retractions: usize,
}

/// Payload for ReconciliationCompleted event (ADR-038).
/// Emitted at the end of each 10-minute reconciliation pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationCompletedPayload {
    /// Number of rules evaluated in this pass
    pub rules_evaluated: usize,
    /// Total new triples asserted across all rules
    pub total_assertions: usize,
    /// Total triples retracted across all rules
    pub total_retractions: usize,
}

// --- Telemetry payloads (ADR-045) ---

/// Payload for TelemetryAggregated event (ADR-045).
/// Emitted by the OTel aggregator every 15s with summary metrics
/// computed from the in-process OTel stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryAggregatedPayload {
    /// Node that produced the aggregation
    pub node_iri: ResourceIri,
    /// Number of spans received in this window
    pub span_count: u64,
    /// Number of metric data points received in this window
    pub metric_count: u64,
    /// Number of log records received in this window
    pub log_count: u64,
    /// Summary metric entries computed from OTel data
    pub summaries: Vec<MetricEntry>,
}

// --- Node failure / workload rescheduling payloads ---

/// Payload for NodeUnreachable event.
/// Emitted when a peer node disappears from mDNS discovery,
/// indicating it has gone down or lost network connectivity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUnreachablePayload {
    /// The node ID that became unreachable
    pub node_id: Uuid,
    /// The node IRI
    pub node_iri: ResourceIri,
    /// The node name
    pub node_name: String,
    /// The last known address of the node
    pub last_address: String,
}

/// Payload for WorkloadRescheduled event.
/// Emitted when a workload is rescheduled from a failed node
/// to a healthy node in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadRescheduledPayload {
    /// IRI of the workload that was rescheduled
    pub workload_iri: ResourceIri,
    /// Node that the workload was previously running on
    pub from_node_iri: ResourceIri,
    /// Reason for rescheduling
    pub reason: String,
}

// --- Telemetry record types (ADR-046) ---

/// A single span record for telemetry storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanRecord {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub service_name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub duration_ms: u64,
    pub status: String,
    pub attributes: serde_json::Value,
}

/// A single metric record for telemetry storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRecord {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub metric_type: String,
    pub service_name: String,
    pub timestamp: DateTime<Utc>,
    pub attributes: serde_json::Value,
}

/// Filter for querying telemetry data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryFilter {
    /// Filter by service name
    pub service_name: Option<String>,
    /// Filter by operation name (spans only)
    pub operation_name: Option<String>,
    /// Filter by metric name (metrics only)
    pub metric_name: Option<String>,
    /// Minimum duration in ms (spans only)
    pub min_duration_ms: Option<u64>,
}

// --- Replay payloads (ADR-035) ---

/// Metadata attached to replayed events to distinguish them from live events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayMetadata {
    /// Whether this event is a replay (always true when present).
    pub is_replay: bool,
    /// The replay operation ID grouping all replayed events.
    pub replay_id: Uuid,
    /// The original timestamp when the event was first written.
    pub original_timestamp: DateTime<Utc>,
    /// When this event was re-emitted during replay.
    pub replayed_at: DateTime<Utc>,
}

/// Payload for ReplayStarted event (ADR-035).
/// Emitted when a replay operation begins building a shadow projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayStartedPayload {
    /// Unique identifier for this replay operation.
    pub replay_id: Uuid,
    /// Product scope (None for platform-level replay).
    pub product: Option<String>,
    /// Start of the replay time range.
    pub from: DateTime<Utc>,
    /// End of the replay time range (None = replay to present).
    pub to: Option<DateTime<Utc>>,
    /// Optional aggregate type filter.
    pub aggregate_type: Option<String>,
    /// Optional aggregate ID filter.
    pub aggregate_ids: Vec<String>,
}

/// Payload for ReplayProgress event (ADR-035).
/// Emitted periodically during replay to report progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayProgressPayload {
    /// The replay operation ID.
    pub replay_id: Uuid,
    /// Number of events processed so far.
    pub events_processed: usize,
    /// Total number of events to replay (if known).
    pub events_total: Option<usize>,
}

/// Payload for ReplayCompleted event (ADR-035).
/// Emitted when the shadow graph is atomically swapped with the live graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCompletedPayload {
    /// The replay operation ID.
    pub replay_id: Uuid,
    /// Total events replayed.
    pub events_replayed: usize,
    /// IRI of the shadow graph that was swapped in.
    pub shadow_graph_iri: String,
}

/// Payload for ReplayFailed event (ADR-035).
/// Emitted when a replay operation fails; the live graph is unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayFailedPayload {
    /// The replay operation ID.
    pub replay_id: Uuid,
    /// Human-readable reason for failure.
    pub reason: String,
    /// Number of events processed before failure.
    pub events_processed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iri::ResourceIri;

    fn make_envelope() -> EventEnvelope {
        let schema = ResourceIri("https://picloud.local/schemas/events/ResourceReady/v1".to_string());
        let source = ResourceIri("https://picloud.local/products/photo-app/containers/api".to_string());
        let correlation = Uuid::new_v4();
        let payload = serde_json::json!({"key": "value"});

        EventEnvelope::new(
            schema,
            "ResourceReady",
            source,
            Some("photo-app".to_string()),
            correlation,
            payload,
        )
    }

    #[test]
    fn new_sets_fields_correctly() {
        let correlation = Uuid::new_v4();
        let schema = ResourceIri("https://picloud.local/schemas/events/ResourceReady/v1".to_string());
        let source = ResourceIri("https://picloud.local/products/photo-app".to_string());
        let payload = serde_json::json!({"status": "ok"});

        let env = EventEnvelope::new(
            schema.clone(),
            "ResourceReady",
            source.clone(),
            Some("photo-app".to_string()),
            correlation,
            payload.clone(),
        );

        assert_eq!(env.schema, schema);
        assert_eq!(env.event_type, "ResourceReady");
        assert_eq!(env.source, source);
        assert_eq!(env.product, Some("photo-app".to_string()));
        assert_eq!(env.correlation_id, correlation);
        assert_eq!(env.payload, payload);
        assert!(env.idempotency_key.is_none());
    }

    #[test]
    fn with_idempotency_key_sets_key() {
        let env = make_envelope().with_idempotency_key("my-key-123");
        assert_eq!(env.idempotency_key, Some("my-key-123".to_string()));
    }

    #[test]
    fn new_generates_unique_ids() {
        let a = make_envelope();
        let b = make_envelope();
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn alert_severity_serde_round_trip() {
        let severities = vec![
            AlertSeverity::Info,
            AlertSeverity::Warning,
            AlertSeverity::Critical,
        ];
        for severity in severities {
            let json = serde_json::to_string(&severity).unwrap();
            let back: AlertSeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, severity);
        }
    }

    #[test]
    fn alert_severity_serializes_as_snake_case() {
        assert_eq!(serde_json::to_string(&AlertSeverity::Info).unwrap(), "\"info\"");
        assert_eq!(serde_json::to_string(&AlertSeverity::Warning).unwrap(), "\"warning\"");
        assert_eq!(serde_json::to_string(&AlertSeverity::Critical).unwrap(), "\"critical\"");
    }

    #[test]
    fn alert_severity_display() {
        assert_eq!(AlertSeverity::Info.to_string(), "info");
        assert_eq!(AlertSeverity::Warning.to_string(), "warning");
        assert_eq!(AlertSeverity::Critical.to_string(), "critical");
    }

    #[test]
    fn alert_fired_payload_serde() {
        let payload = AlertFiredPayload {
            alert_type: "HighCpuTemperature".to_string(),
            severity: AlertSeverity::Critical,
            message: "CPU temperature above 80 C on pi-node-02".to_string(),
            resource_iri: ResourceIri("https://picloud.local/nodes/pi-node-02".to_string()),
            rule_iri: ResourceIri("https://picloud.local/inference-rules/high-cpu-temp-critical".to_string()),
            fired_at: Utc::now(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["alert_type"], "HighCpuTemperature");
        assert_eq!(json["severity"], "critical");
        let back: AlertFiredPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.alert_type, "HighCpuTemperature");
        assert_eq!(back.severity, AlertSeverity::Critical);
    }

    #[test]
    fn alert_resolved_payload_serde() {
        let payload = AlertResolvedPayload {
            alert_type: "HighCpuTemperature".to_string(),
            resource_iri: ResourceIri("https://picloud.local/nodes/pi-node-02".to_string()),
            rule_iri: ResourceIri("https://picloud.local/inference-rules/high-cpu-temp-critical".to_string()),
            resolved_at: Utc::now(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["alert_type"], "HighCpuTemperature");
        let back: AlertResolvedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.alert_type, "HighCpuTemperature");
    }

    #[test]
    fn cluster_initialized_payload_serde() {
        let payload = ClusterInitializedPayload {
            cluster_id: Uuid::new_v4(),
            domain: "picloud.local".to_string(),
            ca_fingerprint: "sha256:abc123".to_string(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["domain"], "picloud.local");
        assert_eq!(json["ca_fingerprint"], "sha256:abc123");
        let back: ClusterInitializedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.domain, "picloud.local");
        assert_eq!(back.cluster_id, payload.cluster_id);
    }

    #[test]
    fn node_join_rejected_payload_serde() {
        let payload = NodeJoinRejectedPayload {
            node_id: Uuid::new_v4(),
            address: "192.168.1.50:7443".to_string(),
            reason: "CA fingerprint mismatch".to_string(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["reason"], "CA fingerprint mismatch");
        let back: NodeJoinRejectedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.reason, "CA fingerprint mismatch");
    }

    #[test]
    fn platform_level_event_has_no_product() {
        let schema = ResourceIri("https://picloud.local/schemas/events/NodeJoined/v1".to_string());
        let source = ResourceIri("https://picloud.local/nodes/pi-01".to_string());

        let env = EventEnvelope::new(
            schema,
            "NodeJoined",
            source,
            None,
            Uuid::new_v4(),
            serde_json::json!({}),
        );

        assert!(env.product.is_none());
    }
}
