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
