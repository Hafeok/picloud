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
