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
    /// W3C traceparent header for distributed trace correlation (FT-048).
    /// Format: `{version}-{trace-id}-{parent-id}-{trace-flags}`
    /// Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    /// Replay metadata — present only when this event was re-emitted during
    /// a replay operation (ADR-035). Subscribers inspect this field to decide
    /// whether to apply side-effects or treat the event as informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<ReplayMetadata>,
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
            traceparent: None,
            replay: None,
            payload,
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Attach a W3C traceparent header to this event (FT-048).
    pub fn with_traceparent(mut self, traceparent: impl Into<String>) -> Self {
        self.traceparent = Some(traceparent.into());
        self
    }

    /// Attach replay metadata to mark this event as a replayed event (ADR-035).
    pub fn with_replay_metadata(mut self, metadata: ReplayMetadata) -> Self {
        self.replay = Some(metadata);
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
    GroupCreated(GroupCreatedPayload),
    GroupRoleAssigned(GroupRoleAssignedPayload),
    GroupRoleRevoked(GroupRoleRevokedPayload),
    GroupDeleted(GroupDeletedPayload),

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

    // --- Snapshot & backup events (ADR-047) ---
    SnapshotCreated(SnapshotCreatedPayload),
    SnapshotFailed(SnapshotFailedPayload),
    SnapshotDeleted(SnapshotDeletedPayload),
    BackupStarted(BackupStartedPayload),
    BackupCompleted(BackupCompletedPayload),
    BackupFailed(BackupFailedPayload),

    // --- Certificate enrollment events (ADR-053) ---
    NodeEnrolled(NodeEnrolledPayload),
    NodeEnrollmentRejected(NodeEnrollmentRejectedPayload),
    CertIssued(CertIssuedPayload),
    CertRenewed(CertRenewedPayload),
    CertRevoked(CertRevokedPayload),
    EnrollmentTokenIssued(EnrollmentTokenIssuedPayload),
    EnrollmentTokenRevoked(EnrollmentTokenRevokedPayload),

    // --- Product IAM events (ADR-051) ---
    RoleAssigned(RoleAssignedPayload),
    RoleRevoked(RoleRevokedPayload),
    TokenExchanged(TokenExchangedPayload),

    // --- OCI Registry events (ADR-054) ---
    ImagePushed(ImagePushedPayload),
    ImageDeleted(ImageDeletedPayload),
    ImageTagUpdated(ImageTagUpdatedPayload),
    RegistryGCStarted(RegistryGCStartedPayload),
    RegistryGCCompleted(RegistryGCCompletedPayload),
    RegistryAuthFailed(RegistryAuthFailedPayload),

    // --- Capability events (ADR-055, FT-062) ---
    CapabilityDeclared(CapabilityDeclaredPayload),
    CapabilityReady(CapabilityReadyPayload),
    CapabilityImplementorAdded(CapabilityImplementorAddedPayload),
    CapabilityImplementorRemoved(CapabilityImplementorRemovedPayload),
    CapabilityConsumerAdded(CapabilityConsumerAddedPayload),
    CapabilityUnfulfilled(CapabilityUnfulfilledPayload),
    CapabilityDeleted(CapabilityDeletedPayload),
    CapabilityRoutingFailed(CapabilityRoutingFailedPayload),

    // --- Data Domain events (ADR-056, FT-071) ---
    DataDomainDeclared(DataDomainDeclaredPayload),
    DataDomainUpdated(DataDomainUpdatedPayload),
    DataDomainDeleted(DataDomainDeletedPayload),

    // --- Data Product events (ADR-056, FT-070) ---
    DataProductDeclared(DataProductDeclaredPayload),
    DataProductUpdated(DataProductUpdatedPayload),
    DataProductReady(DataProductReadyPayload),
    DataProductRefreshed(DataProductRefreshedPayload),
    DataProductSLOBreached(DataProductSLOBreachedPayload),
    DataProductSLORestored(DataProductSLORestoredPayload),
    DataProductDeleted(DataProductDeletedPayload),

    // --- Ontology events (ADR-023, FT-053) ---
    OntologyLoaded(OntologyLoadedPayload),

    // --- Subscription event routing (ADR-022, FT-084) ---
    SubscriptionEventRouted(SubscriptionEventRoutedPayload),

    // --- Node drain events (FT-011) ---
    NodeCordoned(NodeCordonedPayload),
    NodeUncordoned(NodeUncordonedPayload),
    NodeDrainStarted(NodeDrainStartedPayload),
    NodeDrainCompleted(NodeDrainCompletedPayload),
    NodeDrainFailed(NodeDrainFailedPayload),
    WorkloadMigrated(WorkloadMigratedPayload),

    // --- Voter configuration events (FT-095) ---
    VoterAdded(VoterAddedPayload),
    VoterRemoved(VoterRemovedPayload),
    VoterConfigurationChanged(VoterConfigurationChangedPayload),

    // --- Log compaction events (FT-011) ---
    LogCompactionCompleted(LogCompactionCompletedPayload),

    // --- Self-monitoring events (FT-011) ---
    SelfMonitoringCheckCompleted(SelfMonitoringCheckCompletedPayload),
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

/// Payload for GroupCreated event (ADR-037, FT-056).
/// Emitted when a new group resource is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCreatedPayload {
    /// IRI of the newly created group
    pub group_iri: ResourceIri,
    /// Human-readable group name
    pub name: String,
    /// Optional description
    pub description: Option<String>,
}

/// Payload for GroupRoleAssigned event (ADR-037, FT-056).
/// Emitted when a role is assigned to a group. All group members inherit the role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRoleAssignedPayload {
    /// IRI of the group
    pub group_iri: ResourceIri,
    /// Name of the role assigned to the group
    pub role_name: String,
    /// Product scope (if the role is product-scoped)
    pub product: Option<String>,
}

/// Payload for GroupRoleRevoked event (ADR-037, FT-056).
/// Emitted when a role is removed from a group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRoleRevokedPayload {
    /// IRI of the group
    pub group_iri: ResourceIri,
    /// Name of the role revoked from the group
    pub role_name: String,
    /// Product scope
    pub product: Option<String>,
}

/// Payload for GroupDeleted event (ADR-037, FT-056).
/// Emitted when a group resource is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDeletedPayload {
    /// IRI of the deleted group
    pub group_iri: ResourceIri,
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

// --- Node drain payloads (FT-011) ---

/// Payload for NodeCordoned event.
/// Emitted when a node is cordoned — it will not accept new workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCordonedPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub node_name: String,
}

/// Payload for NodeUncordoned event.
/// Emitted when a cordoned node is returned to service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUncordonedPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub node_name: String,
}

/// Payload for NodeDrainStarted event.
/// Emitted when a drain operation begins on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDrainStartedPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub node_name: String,
    /// Number of workloads to evacuate
    pub workload_count: usize,
}

/// Payload for NodeDrainCompleted event.
/// Emitted when all workloads have been successfully evacuated from a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDrainCompletedPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub node_name: String,
    /// Number of workloads that were migrated
    pub workloads_migrated: usize,
    /// Duration of the drain operation in milliseconds
    pub duration_ms: u64,
}

/// Payload for NodeDrainFailed event.
/// Emitted when a drain operation fails (timeout or workload migration error).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDrainFailedPayload {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub node_name: String,
    pub reason: String,
}

/// Payload for WorkloadMigrated event.
/// Emitted when a workload is moved from a draining node to another node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadMigratedPayload {
    pub workload_iri: ResourceIri,
    pub from_node_iri: ResourceIri,
    pub to_node_iri: ResourceIri,
    pub reason: String,
}

// --- Voter configuration payloads (FT-095) ---

/// The role of a node in the Raft cluster.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoterRole {
    /// Full voting member — participates in leader election and quorum.
    Voter,
    /// Learner (non-voter) — receives replicated log entries but does not vote.
    Learner,
}

impl std::fmt::Display for VoterRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoterRole::Voter => write!(f, "voter"),
            VoterRole::Learner => write!(f, "learner"),
        }
    }
}

/// Payload for VoterAdded event (FT-095).
/// Emitted when a node is promoted from learner to voter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoterAddedPayload {
    /// The node ID that was promoted to voter
    pub node_id: Uuid,
    /// The node IRI
    pub node_iri: ResourceIri,
    /// The new voter set size after this change
    pub new_voter_count: usize,
}

/// Payload for VoterRemoved event (FT-095).
/// Emitted when a node is demoted from voter to learner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoterRemovedPayload {
    /// The node ID that was demoted to learner
    pub node_id: Uuid,
    /// The node IRI
    pub node_iri: ResourceIri,
    /// The new voter set size after this change
    pub new_voter_count: usize,
}

/// Payload for VoterConfigurationChanged event (FT-095).
/// Emitted when the voter set is changed atomically via joint consensus.
/// This is the completion event — the new configuration is fully committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoterConfigurationChangedPayload {
    /// The previous voter set (node IDs)
    pub previous_voters: Vec<Uuid>,
    /// The new voter set (node IDs)
    pub new_voters: Vec<Uuid>,
    /// Nodes added as voters in this change
    pub voters_added: Vec<Uuid>,
    /// Nodes removed from the voter set in this change
    pub voters_removed: Vec<Uuid>,
}

// --- Log compaction payloads (FT-011) ---

/// Payload for LogCompactionCompleted event.
/// Emitted when the event log compaction finishes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogCompactionCompletedPayload {
    /// Number of events discarded during compaction
    pub events_discarded: usize,
    /// Number of events remaining after compaction
    pub events_remaining: usize,
    /// The new snapshot offset after compaction
    pub snapshot_offset: usize,
}

// --- Self-monitoring payloads (FT-011) ---

/// Health status for a single self-monitoring check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthStatus::Healthy => write!(f, "healthy"),
            HealthStatus::Degraded => write!(f, "degraded"),
            HealthStatus::Unhealthy => write!(f, "unhealthy"),
        }
    }
}

/// A single check result from the self-monitoring system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfMonitoringCheck {
    /// Name of the check, e.g. "raft_health", "replication_status", "projection_lag"
    pub check_name: String,
    /// Status of the check
    pub status: HealthStatus,
    /// Human-readable message
    pub message: String,
}

/// Payload for SelfMonitoringCheckCompleted event.
/// Emitted periodically by the platform self-monitoring system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfMonitoringCheckCompletedPayload {
    pub node_iri: ResourceIri,
    /// Overall health status (worst of all checks)
    pub overall_status: HealthStatus,
    /// Individual check results
    pub checks: Vec<SelfMonitoringCheck>,
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

// --- Telemetry retention policy types (FT-049 / ADR-046) ---

/// Telemetry signal type — each signal can have its own retention TTL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TelemetrySignalType {
    /// Distributed traces / spans.
    Traces,
    /// Time-series metrics (counters, gauges, histograms).
    Metrics,
    /// Structured log records.
    Logs,
}

impl std::fmt::Display for TelemetrySignalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Traces => write!(f, "traces"),
            Self::Metrics => write!(f, "metrics"),
            Self::Logs => write!(f, "logs"),
        }
    }
}

/// Per-signal retention policy configuration (ADR-046 defaults).
///
/// Each signal type has its own TTL in hours:
/// - Traces: 168 h (7 days)
/// - Metrics: 720 h (30 days)
/// - Logs: 168 h (7 days)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryRetentionPolicy {
    /// Retention TTL for trace spans in hours.
    pub traces_hours: u64,
    /// Retention TTL for metrics in hours.
    pub metrics_hours: u64,
    /// Retention TTL for logs in hours.
    pub logs_hours: u64,
}

impl Default for TelemetryRetentionPolicy {
    fn default() -> Self {
        Self {
            traces_hours: 168,  // 7 days
            metrics_hours: 720, // 30 days
            logs_hours: 168,    // 7 days
        }
    }
}

impl TelemetryRetentionPolicy {
    /// Get the retention TTL in hours for a given signal type.
    pub fn ttl_hours(&self, signal: TelemetrySignalType) -> u64 {
        match signal {
            TelemetrySignalType::Traces => self.traces_hours,
            TelemetrySignalType::Metrics => self.metrics_hours,
            TelemetrySignalType::Logs => self.logs_hours,
        }
    }

    /// Set the retention TTL in hours for a given signal type.
    pub fn set_ttl_hours(&mut self, signal: TelemetrySignalType, hours: u64) {
        match signal {
            TelemetrySignalType::Traces => self.traces_hours = hours,
            TelemetrySignalType::Metrics => self.metrics_hours = hours,
            TelemetrySignalType::Logs => self.logs_hours = hours,
        }
    }
}

/// Result of enforcing a retention policy for a single signal type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionEnforcementResult {
    /// Which signal type was cleaned up.
    pub signal: TelemetrySignalType,
    /// Number of partition directories deleted.
    pub partitions_deleted: u64,
    /// The cutoff timestamp — data older than this was deleted.
    pub cutoff: DateTime<Utc>,
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

// --- Snapshot & backup payloads (ADR-047) ---

/// Payload for SnapshotCreated event.
/// Emitted when a point-in-time snapshot of a volume is successfully taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotCreatedPayload {
    /// IRI of the volume that was snapshotted
    pub volume_iri: ResourceIri,
    /// Path where the snapshot was stored
    pub snapshot_path: String,
    /// Snapshot size in bytes
    pub size_bytes: u64,
    /// When the snapshot was taken
    pub created_at: DateTime<Utc>,
}

/// Payload for SnapshotFailed event.
/// Emitted when a scheduled snapshot fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotFailedPayload {
    /// IRI of the volume that was being snapshotted
    pub volume_iri: ResourceIri,
    /// Human-readable reason for failure
    pub reason: String,
}

/// Payload for SnapshotDeleted event.
/// Emitted when an old snapshot is removed by the retention policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDeletedPayload {
    /// IRI of the volume whose snapshot was deleted
    pub volume_iri: ResourceIri,
    /// Path of the deleted snapshot
    pub snapshot_path: String,
}

/// Payload for BackupStarted event.
/// Emitted when an offsite backup begins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStartedPayload {
    /// IRI of the volume being backed up
    pub volume_iri: ResourceIri,
    /// The snapshot being backed up offsite
    pub snapshot_path: String,
}

/// Payload for BackupCompleted event.
/// Emitted when an offsite backup finishes successfully.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupCompletedPayload {
    /// IRI of the volume that was backed up
    pub volume_iri: ResourceIri,
    /// Size of the backup in bytes
    pub size_bytes: u64,
    /// When the backup completed
    pub completed_at: DateTime<Utc>,
}

/// Payload for BackupFailed event.
/// Emitted when an offsite backup fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFailedPayload {
    /// IRI of the volume that was being backed up
    pub volume_iri: ResourceIri,
    /// Human-readable reason for failure
    pub reason: String,
}

// --- Certificate enrollment payloads (ADR-053) ---

/// The type of certificate issued by the cluster CA.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CertType {
    /// Node-to-node mTLS certificate (90-day lifetime)
    Node,
    /// Workload identity certificate (24-hour lifetime)
    Workload,
    /// Ingress TLS certificate (90-day lifetime)
    Ingress,
}

/// Payload for NodeEnrolled event (ADR-053).
/// Emitted when a new node successfully receives its certificate from the cluster CA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEnrolledPayload {
    pub node_id: Uuid,
    pub node_name: String,
    pub node_address: String,
    pub fingerprint: String,
    pub enrollment_mode: String,
    pub expires_at: DateTime<Utc>,
}

/// Payload for NodeEnrollmentRejected event (ADR-053).
/// Emitted when a node's enrollment request is denied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEnrollmentRejectedPayload {
    pub node_name: String,
    pub node_address: String,
    pub reason: String,
}

/// Payload for CertIssued event (ADR-053).
/// Emitted when the cluster CA issues any certificate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertIssuedPayload {
    pub cert_type: CertType,
    pub subject: String,
    pub fingerprint: String,
    pub expires_at: DateTime<Utc>,
}

/// Payload for CertRenewed event (ADR-053).
/// Emitted when a certificate is automatically renewed before expiry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertRenewedPayload {
    pub cert_type: CertType,
    pub subject: String,
    pub old_fingerprint: String,
    pub new_fingerprint: String,
    pub expires_at: DateTime<Utc>,
}

/// Payload for CertRevoked event (ADR-053).
/// Emitted when a certificate is added to the in-memory CRL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertRevokedPayload {
    pub cert_type: CertType,
    pub subject: String,
    pub fingerprint: String,
    pub reason: String,
}

/// Payload for EnrollmentTokenIssued event (ADR-053).
/// Emitted when a cluster admin generates a new enrollment token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentTokenIssuedPayload {
    pub token_id: Uuid,
    pub created_by: ResourceIri,
    pub expires_at: DateTime<Utc>,
    pub for_node: Option<String>,
}

/// Payload for EnrollmentTokenRevoked event (ADR-053).
/// Emitted when an enrollment token is revoked before use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentTokenRevokedPayload {
    pub token_id: Uuid,
    pub revoked_by: ResourceIri,
}

// --- Product IAM payloads (ADR-051) ---

/// Payload for RoleAssigned event (ADR-051).
/// Emitted when a role is assigned to an identity within a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignedPayload {
    pub identity_iri: ResourceIri,
    pub role_name: String,
    pub product: String,
}

/// Payload for RoleRevoked event (ADR-051).
/// Emitted when a role is removed from an identity within a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleRevokedPayload {
    pub identity_iri: ResourceIri,
    pub role_name: String,
    pub product: String,
}

/// Payload for TokenExchanged event (ADR-051).
/// Emitted when an RFC 8693 token exchange or M2M client_credentials grant succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenExchangedPayload {
    pub subject_iri: ResourceIri,
    pub audience: String,
    pub flow: String,
    pub scopes: Vec<String>,
}

// --- OCI Registry payloads (ADR-054) ---

/// Payload for ImagePushed event (ADR-054).
/// Emitted when an image is successfully pushed to the embedded registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImagePushedPayload {
    pub repository: String,
    pub tag: Option<String>,
    pub digest: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub pushed_by: Option<String>,
}

/// Payload for ImageDeleted event (ADR-054).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDeletedPayload {
    pub repository: String,
    pub digest: String,
    pub deleted_by: Option<String>,
}

/// Payload for ImageTagUpdated event (ADR-054).
/// Emitted when a tag is moved to a new digest (re-push with same tag).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageTagUpdatedPayload {
    pub repository: String,
    pub tag: String,
    pub previous_digest: String,
    pub new_digest: String,
}

/// Payload for RegistryGCStarted event (ADR-054).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryGCStartedPayload {
    pub scheduled_at: DateTime<Utc>,
}

/// Payload for RegistryGCCompleted event (ADR-054).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryGCCompletedPayload {
    pub blobs_deleted: u64,
    pub bytes_reclaimed: u64,
    pub duration_ms: u64,
}

/// Payload for RegistryAuthFailed event (ADR-054).
/// Emitted when an authentication attempt against the registry fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryAuthFailedPayload {
    pub identity: String,
    pub operation: String,
    pub repository: String,
    pub reason: String,
}

// --- Capability payloads (ADR-055) ---

/// Payload for CapabilityDeclared event (ADR-055).
/// Emitted when a capability resource is created and validated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDeclaredPayload {
    pub capability_iri: ResourceIri,
    pub name: String,
    pub version: String,
    pub input_event: String,
    pub output_event: String,
}

/// Payload for CapabilityReady event (ADR-055).
/// Emitted when at least one implementing Product is deployed and conformant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReadyPayload {
    pub capability_iri: ResourceIri,
    pub implementor_product: String,
}

/// Payload for CapabilityImplementorAdded event (ADR-055).
/// Emitted when a Product declares `implements` for an existing capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityImplementorAddedPayload {
    pub capability_iri: ResourceIri,
    pub capability_name: String,
    pub product_iri: ResourceIri,
    pub product_name: String,
    pub version: String,
}

/// Payload for CapabilityImplementorRemoved event (ADR-055).
/// Emitted when an implementing Product is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityImplementorRemovedPayload {
    pub capability_iri: ResourceIri,
    pub capability_name: String,
    pub product_iri: ResourceIri,
    pub product_name: String,
}

/// Payload for CapabilityConsumerAdded event (ADR-055, FT-062).
/// Emitted when a Product declares a dependency on an existing capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConsumerAddedPayload {
    pub capability_iri: ResourceIri,
    pub capability_name: String,
    pub product_iri: ResourceIri,
    pub product_name: String,
    pub min_version: String,
}

/// Payload for CapabilityUnfulfilled event (ADR-055).
/// Emitted when no implementing Product exists for a capability that has consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityUnfulfilledPayload {
    pub capability_iri: ResourceIri,
    pub capability_name: String,
    /// Products that depend on this capability
    pub consumer_products: Vec<String>,
}

/// Payload for CapabilityDeleted event (ADR-055).
/// Emitted when a capability is removed (only when no consumers exist).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDeletedPayload {
    pub capability_iri: ResourceIri,
    pub capability_name: String,
}

/// Payload for CapabilityRoutingFailed event (ADR-055).
/// Emitted when an input event cannot be routed to an implementor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRoutingFailedPayload {
    pub capability_iri: ResourceIri,
    pub input_event_id: Uuid,
    pub reason: String,
}

// --- Data Domain payloads (ADR-056) ---

/// Payload for DataDomainDeclared event (ADR-056).
/// Emitted when a data domain governance boundary is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDomainDeclaredPayload {
    pub domain_iri: ResourceIri,
    pub name: String,
    pub steward: String,
    pub sensitivity: String,
}

/// Payload for DataDomainUpdated event (ADR-056, FT-071).
/// Emitted when a data domain's governance metadata is modified
/// (steward reassignment, sensitivity reclassification, description change).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDomainUpdatedPayload {
    pub domain_iri: ResourceIri,
    pub name: String,
    /// Updated steward — may differ from the original declaration.
    pub steward: String,
    /// Updated sensitivity classification.
    pub sensitivity: String,
    /// Human-readable description of the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Payload for DataDomainDeleted event (ADR-056).
/// Emitted when a data domain is removed (only when no member data products exist).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDomainDeletedPayload {
    pub domain_iri: ResourceIri,
    pub name: String,
}

// --- Data Product payloads (ADR-056) ---

/// Payload for DataProductDeclared event (ADR-056).
/// Emitted when a data product resource is created and validated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductDeclaredPayload {
    pub data_product_iri: ResourceIri,
    pub name: String,
    pub product: String,
    pub domain: String,
    pub version: String,
    /// Freshness SLO — maximum allowed age before the data product is
    /// considered stale (ISO 8601 duration, e.g. "PT15M", "PT1H").
    /// Stored in the RDF graph so the SLO monitor can detect breaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<String>,
}

/// Payload for DataProductUpdated event (ADR-056, FT-070).
/// Emitted when a data product's metadata is modified (version bump,
/// domain reassignment, description change, freshness SLO update).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductUpdatedPayload {
    pub data_product_iri: ResourceIri,
    pub name: String,
    pub product: String,
    /// Updated domain — may differ from the original declaration.
    pub domain: String,
    /// Updated version string.
    pub version: String,
    /// Updated freshness SLO (ISO 8601 duration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<String>,
    /// Human-readable description of the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Payload for DataProductReady event (ADR-056).
/// Emitted when a data product's first projection is complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductReadyPayload {
    pub data_product_iri: ResourceIri,
    pub triple_count: u64,
}

/// Payload for DataProductRefreshed event (ADR-056).
/// Emitted after a successful projection rebuild on a trigger event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductRefreshedPayload {
    pub data_product_iri: ResourceIri,
    pub triple_count: u64,
    pub duration_ms: u64,
    pub trigger_event: String,
    pub refreshed_at: DateTime<Utc>,
}

/// Payload for DataProductSLOBreached event (ADR-056).
/// Emitted when a data product's freshness exceeds its declared maxAge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductSLOBreachedPayload {
    pub data_product_iri: ResourceIri,
    pub max_age: String,
    pub actual_age_seconds: u64,
}

/// Payload for DataProductSLORestored event (ADR-056).
/// Emitted when a breached data product is refreshed and meets its SLO again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductSLORestoredPayload {
    pub data_product_iri: ResourceIri,
    pub refreshed_at: DateTime<Utc>,
}

/// Payload for DataProductDeleted event (ADR-056).
/// Emitted when a data product is removed (only when no consumers exist).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductDeletedPayload {
    pub data_product_iri: ResourceIri,
    pub name: String,
    pub product: String,
}

// --- Ontology payloads (ADR-023, FT-053) ---

/// Payload for OntologyLoaded event.
/// Emitted when an ontology file (.ttl or .shacl) is loaded into the
/// product's RDF graph. The `content` field carries the Turtle/SHACL
/// text and `format` indicates the syntax. Triples are loaded into
/// the product's named graph and RDFS inference is materialised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyLoadedPayload {
    /// IRI of the Ontology resource that was loaded
    pub ontology_iri: ResourceIri,
    /// Product this ontology belongs to
    pub product: String,
    /// Product version the ontology is bound to
    pub version: String,
    /// The RDF content (Turtle or SHACL format)
    pub content: String,
    /// File format: "turtle" or "shacl"
    pub format: String,
}

// --- Subscription event routing payloads (ADR-022, FT-084) ---

/// Payload for SubscriptionEventRouted event (ADR-022, FT-084).
/// Emitted when the platform routes an event from a source product to a
/// subscriber product based on an active EventSubscription resource.
/// The subscriber receives this event scoped to its own product namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionEventRoutedPayload {
    /// IRI of the EventSubscription resource that triggered this routing
    pub subscription_iri: ResourceIri,
    /// Name of the source product that emitted the original event
    pub source_product: String,
    /// Name of the subscriber product receiving the routed event
    pub subscriber_product: String,
    /// The handler (container/binary) in the subscriber product that should process this
    pub handler_name: String,
    /// The original event type that was matched
    pub original_event_type: String,
    /// The original event ID
    pub original_event_id: Uuid,
    /// The original event's payload, preserved verbatim
    pub original_payload: serde_json::Value,
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

    #[test]
    fn snapshot_created_payload_serde() {
        let payload = SnapshotCreatedPayload {
            volume_iri: ResourceIri("https://picloud.local/products/photo-app/volumes/media".to_string()),
            snapshot_path: "/snapshots/media/2026-04-07T10:00:00Z".to_string(),
            size_bytes: 1024 * 1024 * 500,
            created_at: Utc::now(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert!(json["snapshot_path"].as_str().unwrap().contains("2026"));
        let back: SnapshotCreatedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.size_bytes, 1024 * 1024 * 500);
    }

    #[test]
    fn snapshot_failed_payload_serde() {
        let payload = SnapshotFailedPayload {
            volume_iri: ResourceIri("https://picloud.local/products/photo-app/volumes/media".to_string()),
            reason: "NAS unreachable".to_string(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["reason"], "NAS unreachable");
        let back: SnapshotFailedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.reason, "NAS unreachable");
    }

    #[test]
    fn backup_completed_payload_serde() {
        let payload = BackupCompletedPayload {
            volume_iri: ResourceIri("https://picloud.local/products/photo-app/volumes/media".to_string()),
            size_bytes: 1024 * 1024 * 200,
            completed_at: Utc::now(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        let back: BackupCompletedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back.size_bytes, 1024 * 1024 * 200);
    }
}
