/// Resource Types
///
/// Every platform concept has a typed representation here.
/// These are the nouns of the platform — what can be created,
/// queried, and deleted via the resource API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::iri::ResourceIri;
use crate::storage::StorageIntent;
use crate::workload::{ContainerSpec, BinarySpec};

/// The lifecycle state of any resource
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    Declared,
    Provisioning,
    Ready,
    Failed,
    Deleting,
}

/// A key-value tag attached to any resource (ADR-036)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Tag {
    pub key: String,
    pub value: String,
}

/// Common metadata carried by every resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeta {
    pub iri: ResourceIri,
    pub resource_type: String,
    pub name: String,
    pub product: Option<String>,
    pub status: ResourceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Free-form key-value tags (ADR-036)
    #[serde(default)]
    pub tags: Vec<Tag>,
}

/// A Product — the top-level deployment unit (ADR-016)
/// Hermetically sealed: owns all its resources, no sharing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub meta: ResourceMeta,
    pub version: String,
    pub description: Option<String>,
}

/// A Volume — block storage with declared intent (ADR-024)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub meta: ResourceMeta,
    pub size_gb: u64,
    pub storage_intent: StorageIntent,
    pub volume_type: VolumeType,
    /// Which node currently holds the primary replica
    pub primary_node_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolumeType {
    /// Presented as a filesystem path inside the workload
    Mounted,
    /// Presented as a raw block device
    RawBlock,
}

/// A Container workload — OCI image scheduled on the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Container {
    pub meta: ResourceMeta,
    pub spec: ContainerSpec,
    /// Node the container is currently scheduled on
    pub node_id: Option<Uuid>,
}

/// A Binary workload — raw ARM64 executable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binary {
    pub meta: ResourceMeta,
    pub spec: BinarySpec,
    pub node_id: Option<Uuid>,
}

/// A managed Oxigraph RDF store — per-product SPARQL endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdfStore {
    pub meta: ResourceMeta,
    pub sparql_endpoint: ResourceIri,
    pub backing_volume: ResourceIri,
}

/// A managed event store — per-product event sourcing (ADR-032)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStore {
    pub meta: ResourceMeta,
    pub aggregates: Vec<AggregateDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateDefinition {
    pub aggregate_type: String,
    /// Path to the .ttl or .shacl schema file, relative to deployment root
    pub schema_file: String,
    /// The IRI where the schema is served once deployed
    pub schema_iri: Option<ResourceIri>,
}

/// An event subscription — inter-product event routing (ADR-022)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscription {
    pub meta: ResourceMeta,
    /// IRI of the source product
    pub source_product_iri: ResourceIri,
    /// Event type to subscribe to
    pub event_type: String,
    /// Container or binary in this product that receives the event
    pub handler_name: String,
}

/// An ingress — exposes a workload path on the cluster domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ingress {
    pub meta: ResourceMeta,
    pub target_name: String,
    pub port: u16,
    pub path: String,
    pub tls: bool,
}

/// An ontology — .ttl or .shacl file bound to a product version (ADR-023)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ontology {
    pub meta: ResourceMeta,
    pub file_path: String,
    pub format: OntologyFormat,
    pub served_at: ResourceIri,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OntologyFormat {
    Turtle,
    Shacl,
}

/// A Secret — an encrypted value injected into workloads at runtime
///
/// Secrets are first-class resources. They are encrypted at rest,
/// replicated across the cluster, and injected into workloads by the
/// platform. Workloads reference secrets by name in env declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secret {
    pub meta: ResourceMeta,
    /// The encrypted value (base64-encoded ciphertext)
    pub encrypted_value: String,
    /// Key ID used for encryption (for key rotation)
    pub key_id: String,
}

/// A Role — an RBAC role with permissions
///
/// Roles can be platform-scoped or product-scoped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub meta: ResourceMeta,
    /// Product scope — None for platform-level roles
    pub product: Option<String>,
    /// List of permission strings (e.g. "photo-app/containers/api-server:read")
    pub permissions: Vec<String>,
}

/// A Group — a collection of identities with shared permissions (ADR-036)
///
/// Groups can be platform-scoped or product-scoped. A user's effective
/// permissions are the union of their direct permissions and all group
/// permissions. Users can be added explicitly or automatically via
/// GroupMembershipRules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub meta: ResourceMeta,
    pub description: Option<String>,
    /// Product scope — None for platform-level groups
    pub product: Option<String>,
    /// Permission strings, same format as Role permissions
    pub permissions: Vec<String>,
}

/// A tag condition used in group membership rules (ADR-036)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagCondition {
    pub key: String,
    /// If None, matches any value for the given key
    pub value: Option<String>,
}

/// A GroupMembershipRule — tag-based automatic group membership (ADR-036)
///
/// When all tag conditions match an identity's tags, that identity is
/// automatically added to the target group. The RDF projector materializes
/// these inferred memberships as `picloud:memberOf` triples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMembershipRule {
    pub meta: ResourceMeta,
    /// The group this rule adds members to
    pub group_iri: ResourceIri,
    pub description: Option<String>,
    /// All conditions must match (AND logic). Multiple rules on the same
    /// group provide OR logic.
    pub tag_conditions: Vec<TagCondition>,
}

/// A Node — a cluster member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub meta: ResourceMeta,
    pub node_id: Uuid,
    pub address: String,
    pub is_leader: bool,
    pub storage_capacity_gb: u64,
    pub storage_used_gb: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_status_serde_round_trip() {
        let statuses = vec![
            ResourceStatus::Declared,
            ResourceStatus::Provisioning,
            ResourceStatus::Ready,
            ResourceStatus::Failed,
            ResourceStatus::Deleting,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let back: ResourceStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn resource_status_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&ResourceStatus::Declared).unwrap(),
            "\"declared\""
        );
        assert_eq!(
            serde_json::to_string(&ResourceStatus::Provisioning).unwrap(),
            "\"provisioning\""
        );
        assert_eq!(
            serde_json::to_string(&ResourceStatus::Ready).unwrap(),
            "\"ready\""
        );
        assert_eq!(
            serde_json::to_string(&ResourceStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&ResourceStatus::Deleting).unwrap(),
            "\"deleting\""
        );
    }

    #[test]
    fn resource_status_deserializes_from_snake_case() {
        let ready: ResourceStatus = serde_json::from_str("\"ready\"").unwrap();
        assert_eq!(ready, ResourceStatus::Ready);
    }

    #[test]
    fn volume_type_serde_round_trip() {
        let json = serde_json::to_string(&VolumeType::Mounted).unwrap();
        let back: VolumeType = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, VolumeType::Mounted));

        let json = serde_json::to_string(&VolumeType::RawBlock).unwrap();
        let back: VolumeType = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, VolumeType::RawBlock));
    }

    #[test]
    fn ontology_format_serde_round_trip() {
        let json = serde_json::to_string(&OntologyFormat::Turtle).unwrap();
        let back: OntologyFormat = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, OntologyFormat::Turtle));
    }

    #[test]
    fn tag_serde_round_trip() {
        let tag = Tag {
            key: "department".to_string(),
            value: "engineering".to_string(),
        };
        let json = serde_json::to_string(&tag).unwrap();
        let back: Tag = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tag);
    }

    #[test]
    fn resource_meta_tags_default_empty() {
        let json = r#"{
            "iri": "https://picloud.local/test",
            "resource_type": "Test",
            "name": "test",
            "product": null,
            "status": "ready",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let meta: ResourceMeta = serde_json::from_str(json).unwrap();
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn tag_condition_with_value() {
        let cond = TagCondition {
            key: "env".to_string(),
            value: Some("prod".to_string()),
        };
        let json = serde_json::to_string(&cond).unwrap();
        let back: TagCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "env");
        assert_eq!(back.value, Some("prod".to_string()));
    }

    #[test]
    fn tag_condition_key_only() {
        let cond = TagCondition {
            key: "has-gpu".to_string(),
            value: None,
        };
        let json = serde_json::to_string(&cond).unwrap();
        let back: TagCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "has-gpu");
        assert_eq!(back.value, None);
    }
}
