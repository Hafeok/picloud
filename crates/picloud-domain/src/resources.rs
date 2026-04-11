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

/// A tag — a key:value label attached to any resource (ADR-036).
///
/// Tags are the universal labelling mechanism for the platform.
/// They travel through the event log as TagAdded / TagRemoved events
/// and are projected into the RDF graph for SPARQL querying.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Tag {
    pub key: String,
    pub value: String,
}

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
    /// Tags attached to this resource (ADR-036)
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
    /// Capabilities this product implements (ADR-055), e.g. ["gps-to-place@1.0.0"]
    #[serde(default)]
    pub implements: Vec<CapabilityImplementsRef>,
    /// Capabilities this product depends on (ADR-055)
    #[serde(default)]
    pub capabilities: Vec<CapabilityDependency>,
    /// Data products this product consumes (ADR-056)
    #[serde(default)]
    pub data_products: Vec<DataProductDependency>,
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

/// The type of a configuration value (ADR-043)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigType {
    String,
    Int,
    Float,
    Bool,
    Json,
}

/// A single configuration entry — typed key-value pair with tags (ADR-043)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub config_type: ConfigType,
    #[serde(default)]
    pub tags: std::collections::HashMap<String, String>,
}

/// A managed configuration store for a Product (ADR-043)
///
/// Central key-value store for runtime configuration. Workloads can
/// declare overrides that merge over the product config (workload wins).
/// Changes emit `ConfigChanged` events for live reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigStore {
    pub meta: ResourceMeta,
    pub entries: Vec<ConfigEntry>,
}

/// Version expression operator for feature flags (ADR-044)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VersionOp {
    Eq(u64),
    Gt(u64),
    Gte(u64),
    Lt(u64),
    Lte(u64),
    Range(u64, u64),
}

impl VersionOp {
    /// Parse a version expression string like "= 2", ">= 3", "2..4"
    pub fn parse(expr: &str) -> Option<Self> {
        let expr = expr.trim();
        if let Some(range) = expr.split_once("..") {
            let lo = range.0.trim().parse::<u64>().ok()?;
            let hi = range.1.trim().parse::<u64>().ok()?;
            return Some(VersionOp::Range(lo, hi));
        }
        if let Some(n) = expr.strip_prefix(">=") {
            return Some(VersionOp::Gte(n.trim().parse().ok()?));
        }
        if let Some(n) = expr.strip_prefix(">") {
            return Some(VersionOp::Gt(n.trim().parse().ok()?));
        }
        if let Some(n) = expr.strip_prefix("<=") {
            return Some(VersionOp::Lte(n.trim().parse().ok()?));
        }
        if let Some(n) = expr.strip_prefix("<") {
            return Some(VersionOp::Lt(n.trim().parse().ok()?));
        }
        if let Some(n) = expr.strip_prefix("=") {
            return Some(VersionOp::Eq(n.trim().parse().ok()?));
        }
        // bare number = exact match
        expr.parse::<u64>().ok().map(VersionOp::Eq)
    }

    /// Evaluate the version expression against a major version number.
    pub fn matches(&self, major: u64) -> bool {
        match self {
            VersionOp::Eq(v) => major == *v,
            VersionOp::Gt(v) => major > *v,
            VersionOp::Gte(v) => major >= *v,
            VersionOp::Lt(v) => major < *v,
            VersionOp::Lte(v) => major <= *v,
            VersionOp::Range(lo, hi) => major >= *lo && major <= *hi,
        }
    }
}

impl std::fmt::Display for VersionOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionOp::Eq(v) => write!(f, "= {v}"),
            VersionOp::Gt(v) => write!(f, "> {v}"),
            VersionOp::Gte(v) => write!(f, ">= {v}"),
            VersionOp::Lt(v) => write!(f, "< {v}"),
            VersionOp::Lte(v) => write!(f, "<= {v}"),
            VersionOp::Range(lo, hi) => write!(f, "{lo}..{hi}"),
        }
    }
}

/// Extract the major version from a semver-like string.
/// "2.1.0" -> 2, "3" -> 3
pub fn parse_major_version(version: &str) -> Option<u64> {
    version.split('.').next()?.parse().ok()
}

/// A feature flag bound to a Product version (ADR-044)
///
/// Flags are evaluated against the running Product version. A flag
/// is active when `enabled` is true AND the version expression matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub meta: ResourceMeta,
    pub description: Option<String>,
    pub enabled: bool,
    /// Version expression string, e.g. "= 2", ">= 3", "2..4"
    pub version_expr: String,
}

impl FeatureFlag {
    /// Evaluate whether this flag is active for a given product version string.
    pub fn is_active(&self, product_version: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(major) = parse_major_version(product_version) else {
            return false;
        };
        let Some(op) = VersionOp::parse(&self.version_expr) else {
            return false;
        };
        op.matches(major)
    }
}

/// A Group — an IAM resource for role inheritance (ADR-037)
///
/// Users in a group inherit all roles assigned to the group.
/// Membership is managed via SPARQL CONSTRUCT inference rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub meta: ResourceMeta,
    pub description: Option<String>,
    /// Roles inherited by all members of this group
    pub roles: Vec<String>,
}

/// An Inference Rule — a SPARQL CONSTRUCT query as a platform resource (ADR-038)
///
/// Rules are declared in `.picloud` files, stored in the RDF graph, and
/// evaluated by the inference engine. They produce triples that are
/// written back to the graph; new/removed triples emit events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRule {
    pub meta: ResourceMeta,
    pub description: Option<String>,
    /// Scope: "platform" or a product name
    pub scope: String,
    /// Trigger mode: "event" or "reconciliation"
    pub trigger: String,
    /// Event types that trigger this rule (when trigger = "event")
    #[serde(default)]
    pub trigger_events: Vec<String>,
    /// Whether to also run on the 10-minute reconciliation schedule
    pub reconciliation: bool,
    /// The SPARQL CONSTRUCT query
    pub construct_query: String,
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

/// Cluster identity — established at `cluster init`, immutable after (ADR-042).
///
/// The dual boundary: `domain` prevents accidental mDNS cross-discovery,
/// `cluster_id` + `ca_fingerprint` prevent deliberate or accidental cross-join.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterIdentity {
    /// Unique cluster identifier (UUID generated at init)
    pub cluster_id: Uuid,
    /// Human-readable cluster domain (e.g. "picloud.local")
    pub domain: crate::iri::ClusterDomain,
    /// When the cluster was initialized
    pub created_at: DateTime<Utc>,
    /// SHA-256 fingerprint of the cluster CA certificate
    pub ca_fingerprint: String,
    /// Node enrollment mode — immutable after init (ADR-053)
    #[serde(default)]
    pub enrollment_mode: crate::identity::EnrollmentMode,
}

/// An active alert — produced by inference rules (ADR-041).
///
/// Alert triples live in the RDF graph. This struct is the domain
/// representation returned by `picloud alerts list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Type of alert, e.g. "HighCpuTemperature"
    pub alert_type: String,
    /// Severity: info, warning, or critical
    pub severity: crate::events::AlertSeverity,
    /// Human-readable message
    pub message: String,
    /// IRI of the resource this alert is about
    pub resource_iri: crate::iri::ResourceIri,
    /// When the alert was fired
    pub fired_at: DateTime<Utc>,
}

/// A built-in platform alert rule definition (ADR-041).
///
/// These are registered at startup and evaluated by the inference engine.
#[derive(Debug, Clone)]
pub struct BuiltInAlertRule {
    /// Rule name, used to derive the rule IRI
    pub name: &'static str,
    /// Metric name to check (e.g. "cpu_temp_celsius")
    pub metric_name: &'static str,
    /// Threshold value — alert fires when metric exceeds this
    pub threshold: f64,
    /// Alert type name (e.g. "HighCpuTemperature")
    pub alert_type: &'static str,
    /// Alert severity
    pub severity: crate::events::AlertSeverity,
    /// Human-readable message template (use {value} and {threshold} placeholders)
    pub message_template: &'static str,
}

/// The set of built-in platform alert rules shipped with PiCloud (ADR-041).
pub fn builtin_alert_rules() -> Vec<BuiltInAlertRule> {
    use crate::events::AlertSeverity;
    vec![
        BuiltInAlertRule {
            name: "high-cpu-temp-critical",
            metric_name: "cpu_temp_celsius",
            threshold: 80.0,
            alert_type: "HighCpuTemperature",
            severity: AlertSeverity::Critical,
            message_template: "CPU temperature above 80 C",
        },
        BuiltInAlertRule {
            name: "high-cpu-temp-warning",
            metric_name: "cpu_temp_celsius",
            threshold: 70.0,
            alert_type: "HighCpuTemperature",
            severity: AlertSeverity::Warning,
            message_template: "CPU temperature above 70 C",
        },
        BuiltInAlertRule {
            name: "high-memory-critical",
            metric_name: "memory_used_percent",
            threshold: 90.0,
            alert_type: "HighMemoryUsage",
            severity: AlertSeverity::Critical,
            message_template: "Memory usage above 90%",
        },
        BuiltInAlertRule {
            name: "high-memory-warning",
            metric_name: "memory_used_percent",
            threshold: 80.0,
            alert_type: "HighMemoryUsage",
            severity: AlertSeverity::Warning,
            message_template: "Memory usage above 80%",
        },
        BuiltInAlertRule {
            name: "high-disk-critical",
            metric_name: "disk_used_percent",
            threshold: 90.0,
            alert_type: "HighDiskUsage",
            severity: AlertSeverity::Critical,
            message_template: "Disk usage above 90%",
        },
    ]
}

// --- Capability resources (ADR-055) ---

/// A Capability — a cluster-scoped interface contract (ADR-055)
///
/// A capability is a pure interface declaration: a named, versioned contract
/// expressed as an event schema and SHACL shapes. It has no workload, no
/// container, no code. Products separately declare `implements` (they fulfil
/// the contract) or `capabilities` (they depend on the contract being available).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub meta: ResourceMeta,
    pub version: String,
    pub description: Option<String>,
    /// Path to the OWL ontology file for this capability
    pub ontology: Option<String>,
    /// Path to the SHACL shapes file for this capability
    pub shapes: Option<String>,
    /// The event type that consumers emit to invoke the capability
    pub input_event: String,
    /// The event type that implementors emit as the result
    pub output_event: String,
}

/// A capability dependency declared by a consuming Product (ADR-055)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDependency {
    /// Name of the required capability
    pub capability: String,
    /// Minimum version required (semver)
    pub min_version: String,
}

/// A capability implementation reference: "capability_name@version"
pub type CapabilityImplementsRef = String;

// --- Data Domain / Data Product resources (ADR-056) ---

/// Sensitivity classification for a data domain (ADR-056)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataSensitivity {
    Public,
    Internal,
    Confidential,
    Restricted,
}

/// A Data Domain — a cluster-scoped governance boundary (ADR-056)
///
/// Groups related data products across Products. Declares a steward identity,
/// sensitivity classification, and domain-level constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDomain {
    pub meta: ResourceMeta,
    pub description: Option<String>,
    /// IRI of the steward identity responsible for this domain
    pub steward: String,
    /// Data sensitivity classification
    pub sensitivity: DataSensitivity,
}

/// Freshness SLO for a data product (ADR-056)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreshnessConfig {
    /// Maximum age before the data product is considered stale (e.g. "15m", "1h")
    pub max_age: String,
    /// Event types that trigger a projection rebuild
    pub triggers: Vec<String>,
}

/// Access configuration for a data product (ADR-056)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductAccess {
    /// Visibility: "cluster" or "product"
    #[serde(default = "default_visibility")]
    pub visibility: String,
    /// Roles required to access this data product
    #[serde(default)]
    pub roles: Vec<String>,
}

fn default_visibility() -> String {
    "cluster".to_string()
}

/// A Data Product — a published analytical projection (ADR-056)
///
/// A data product publishes a curated, versioned projection of selected
/// domain data into a separate named graph. Push-only: rebuilds on
/// declared trigger events via SPARQL CONSTRUCT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProduct {
    pub meta: ResourceMeta,
    /// The product that owns this data product
    pub product: String,
    /// The data domain this data product belongs to
    pub domain: String,
    pub version: String,
    pub description: Option<String>,
    /// Path to the OWL ontology file
    pub ontology: Option<String>,
    /// Path to the SHACL shapes file
    pub shapes: Option<String>,
    /// Path to the SPARQL CONSTRUCT query file
    pub projection: String,
    /// Freshness SLO configuration
    pub freshness: FreshnessConfig,
    /// Access control configuration
    pub access: DataProductAccess,
}

/// A data product dependency declared by a consuming Product (ADR-056)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataProductDependency {
    /// Source in "product/data-product-name" format
    pub source: String,
    /// Minimum version required
    pub min_version: String,
}

// --- OCI Registry resources (ADR-054) ---

/// The embedded OCI registry — a first-class platform component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Registry {
    pub meta: ResourceMeta,
    /// HTTP endpoint (e.g. "registry.picloud.local")
    pub endpoint: String,
    /// Total storage used by blobs in bytes
    pub storage_used_bytes: u64,
    /// Number of content-addressed blobs
    pub blob_count: u64,
    /// Number of image repositories
    pub repository_count: u64,
}

/// An image repository within the registry, namespaced by product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRepository {
    pub meta: ResourceMeta,
    /// Product that owns this repository
    pub product: String,
    /// Repository name (e.g. "api-server")
    pub name: String,
    /// Number of tags in this repository
    pub tag_count: u64,
}

/// A tagged image in a repository, pointing to a content-addressed digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageTag {
    pub meta: ResourceMeta,
    /// Tag name (e.g. "1.0.0", "latest")
    pub tag: String,
    /// Content digest (e.g. "sha256:e3b0c44...")
    pub digest: String,
    /// Image size in bytes
    pub size_bytes: u64,
    /// OCI media type
    pub media_type: String,
    /// When this tag was pushed
    pub pushed_at: DateTime<Utc>,
    /// Identity that pushed this tag
    pub pushed_by: Option<String>,
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
    fn config_type_serde_round_trip() {
        let types = vec![
            ConfigType::String,
            ConfigType::Int,
            ConfigType::Float,
            ConfigType::Bool,
            ConfigType::Json,
        ];
        for ct in types {
            let json = serde_json::to_string(&ct).unwrap();
            let back: ConfigType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, ct);
        }
    }

    #[test]
    fn config_entry_serde() {
        let entry = ConfigEntry {
            key: "cache.ttl".to_string(),
            value: "300".to_string(),
            config_type: ConfigType::Int,
            tags: [("tier".to_string(), "cache".to_string())].into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: ConfigEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "cache.ttl");
        assert_eq!(back.config_type, ConfigType::Int);
        assert_eq!(back.tags.get("tier").unwrap(), "cache");
    }

    #[test]
    fn version_op_parse_exact() {
        assert_eq!(VersionOp::parse("= 2"), Some(VersionOp::Eq(2)));
    }

    #[test]
    fn version_op_parse_gte() {
        assert_eq!(VersionOp::parse(">= 3"), Some(VersionOp::Gte(3)));
    }

    #[test]
    fn version_op_parse_gt() {
        assert_eq!(VersionOp::parse("> 1"), Some(VersionOp::Gt(1)));
    }

    #[test]
    fn version_op_parse_lt() {
        assert_eq!(VersionOp::parse("< 5"), Some(VersionOp::Lt(5)));
    }

    #[test]
    fn version_op_parse_lte() {
        assert_eq!(VersionOp::parse("<= 4"), Some(VersionOp::Lte(4)));
    }

    #[test]
    fn version_op_parse_range() {
        assert_eq!(VersionOp::parse("2..4"), Some(VersionOp::Range(2, 4)));
    }

    #[test]
    fn version_op_parse_bare_number() {
        assert_eq!(VersionOp::parse("7"), Some(VersionOp::Eq(7)));
    }

    #[test]
    fn version_op_parse_invalid() {
        assert!(VersionOp::parse("abc").is_none());
    }

    #[test]
    fn version_op_matches_exact() {
        assert!(VersionOp::Eq(2).matches(2));
        assert!(!VersionOp::Eq(2).matches(3));
    }

    #[test]
    fn version_op_matches_range_inclusive() {
        let op = VersionOp::Range(2, 4);
        assert!(!op.matches(1));
        assert!(op.matches(2));
        assert!(op.matches(3));
        assert!(op.matches(4));
        assert!(!op.matches(5));
    }

    #[test]
    fn version_op_matches_gte() {
        assert!(VersionOp::Gte(2).matches(2));
        assert!(VersionOp::Gte(2).matches(3));
        assert!(!VersionOp::Gte(2).matches(1));
    }

    #[test]
    fn version_op_matches_lt() {
        assert!(VersionOp::Lt(2).matches(1));
        assert!(!VersionOp::Lt(2).matches(2));
    }

    #[test]
    fn parse_major_version_semver() {
        assert_eq!(parse_major_version("2.1.0"), Some(2));
        assert_eq!(parse_major_version("10.0.1"), Some(10));
        assert_eq!(parse_major_version("3"), Some(3));
    }

    #[test]
    fn feature_flag_is_active_exact_match() {
        let flag = FeatureFlag {
            meta: ResourceMeta {
                iri: ResourceIri("https://picloud.local/products/app/flags/f1".to_string()),
                resource_type: "FeatureFlag".to_string(),
                name: "f1".to_string(),
                product: Some("app".to_string()),
                status: ResourceStatus::Ready,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: Vec::new(),
            },
            description: Some("test".to_string()),
            enabled: true,
            version_expr: "= 2".to_string(),
        };
        assert!(flag.is_active("2.1.0"));
        assert!(!flag.is_active("1.5.0"));
        assert!(!flag.is_active("3.0.0"));
    }

    #[test]
    fn feature_flag_disabled_never_active() {
        let flag = FeatureFlag {
            meta: ResourceMeta {
                iri: ResourceIri("https://picloud.local/products/app/flags/f2".to_string()),
                resource_type: "FeatureFlag".to_string(),
                name: "f2".to_string(),
                product: Some("app".to_string()),
                status: ResourceStatus::Ready,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                tags: Vec::new(),
            },
            description: None,
            enabled: false,
            version_expr: ">= 1".to_string(),
        };
        assert!(!flag.is_active("5.0.0"));
    }

    #[test]
    fn version_op_display() {
        assert_eq!(VersionOp::Eq(2).to_string(), "= 2");
        assert_eq!(VersionOp::Gte(3).to_string(), ">= 3");
        assert_eq!(VersionOp::Range(2, 4).to_string(), "2..4");
    }

    #[test]
    fn tag_serde_round_trip() {
        let tag = Tag {
            key: "team".to_string(),
            value: "backend".to_string(),
        };
        let json = serde_json::to_string(&tag).unwrap();
        let back: Tag = serde_json::from_str(&json).unwrap();
        assert_eq!(back.key, "team");
        assert_eq!(back.value, "backend");
    }

    #[test]
    fn tag_equality() {
        let a = Tag { key: "env".to_string(), value: "prod".to_string() };
        let b = Tag { key: "env".to_string(), value: "prod".to_string() };
        let c = Tag { key: "env".to_string(), value: "dev".to_string() };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn resource_meta_tags_default_empty() {
        let json = r#"{
            "iri": "https://picloud.local/products/test",
            "resource_type": "product",
            "name": "test",
            "product": null,
            "status": "declared",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let meta: ResourceMeta = serde_json::from_str(json).unwrap();
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn resource_meta_tags_deserialize() {
        let json = r#"{
            "iri": "https://picloud.local/products/test",
            "resource_type": "product",
            "name": "test",
            "product": null,
            "status": "declared",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "tags": [{"key": "team", "value": "backend"}]
        }"#;
        let meta: ResourceMeta = serde_json::from_str(json).unwrap();
        assert_eq!(meta.tags.len(), 1);
        assert_eq!(meta.tags[0].key, "team");
        assert_eq!(meta.tags[0].value, "backend");
    }

    #[test]
    fn cluster_identity_serde_round_trip() {
        let identity = ClusterIdentity {
            cluster_id: Uuid::new_v4(),
            domain: crate::iri::ClusterDomain("acme.local".to_string()),
            created_at: Utc::now(),
            ca_fingerprint: "sha256:deadbeef".to_string(),
            enrollment_mode: crate::identity::EnrollmentMode::Token,
        };
        let json = serde_json::to_string(&identity).unwrap();
        let back: ClusterIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cluster_id, identity.cluster_id);
        assert_eq!(back.domain.0, "acme.local");
        assert_eq!(back.ca_fingerprint, "sha256:deadbeef");
        assert_eq!(back.enrollment_mode, crate::identity::EnrollmentMode::Token);
    }

    #[test]
    fn cluster_identity_defaults_enrollment_mode_to_auto() {
        // Backward compatibility: existing serialized data without enrollment_mode
        let json = r#"{
            "cluster_id": "00000000-0000-0000-0000-000000000001",
            "domain": "picloud.local",
            "created_at": "2026-01-01T00:00:00Z",
            "ca_fingerprint": "sha256:abc"
        }"#;
        let back: ClusterIdentity = serde_json::from_str(json).unwrap();
        assert_eq!(back.enrollment_mode, crate::identity::EnrollmentMode::Auto);
    }

    #[test]
    fn alert_serde_round_trip() {
        use crate::events::AlertSeverity;
        let alert = Alert {
            alert_type: "HighCpuTemperature".to_string(),
            severity: AlertSeverity::Critical,
            message: "CPU temperature above 80 C".to_string(),
            resource_iri: crate::iri::ResourceIri("https://picloud.local/nodes/pi-01".to_string()),
            fired_at: Utc::now(),
        };
        let json = serde_json::to_string(&alert).unwrap();
        let back: Alert = serde_json::from_str(&json).unwrap();
        assert_eq!(back.alert_type, "HighCpuTemperature");
        assert_eq!(back.severity, AlertSeverity::Critical);
    }

    #[test]
    fn builtin_alert_rules_are_populated() {
        let rules = builtin_alert_rules();
        assert_eq!(rules.len(), 5);
        // Verify critical CPU temp rule
        assert_eq!(rules[0].name, "high-cpu-temp-critical");
        assert_eq!(rules[0].threshold, 80.0);
        // Verify warning CPU temp rule
        assert_eq!(rules[1].name, "high-cpu-temp-warning");
        assert_eq!(rules[1].threshold, 70.0);
        // Verify critical memory rule
        assert_eq!(rules[2].name, "high-memory-critical");
        assert_eq!(rules[2].threshold, 90.0);
        // Verify warning memory rule
        assert_eq!(rules[3].name, "high-memory-warning");
        assert_eq!(rules[3].threshold, 80.0);
        // Verify critical disk rule
        assert_eq!(rules[4].name, "high-disk-critical");
        assert_eq!(rules[4].threshold, 90.0);
    }
}
