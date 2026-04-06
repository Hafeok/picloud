/// Domain Traits
///
/// These are the abstractions that slices implement.
/// Slices depend only on these traits, never on each other's concrete types.
/// The composition root (picloud binary) wires implementations together.
///
/// This is how vertical slice architecture is enforced at the type level.

use async_trait::async_trait;
use uuid::Uuid;
use crate::error::Result;
use crate::events::EventEnvelope;
use crate::iri::ResourceIri;

// ---- Event Log ----

/// Append events to the Raft-replicated log and subscribe to the stream.
/// Implemented by: picloud-events
#[async_trait]
pub trait EventLog: Send + Sync {
    /// Append an event. Idempotent via idempotency_key.
    async fn append(&self, event: EventEnvelope) -> Result<()>;

    /// Subscribe to all events — returns a stream channel receiver.
    /// Filter by correlation_id to track a specific command.
    async fn subscribe(
        &self,
        filter: EventFilter,
    ) -> Result<tokio::sync::broadcast::Receiver<EventEnvelope>>;
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub correlation_id: Option<Uuid>,
    pub product: Option<String>,
    pub event_types: Vec<String>,
}

// ---- State Projection ----

/// Projects events into the RDF graph read model.
/// Implemented by: picloud-rdf
#[async_trait]
pub trait StateProjector: Send + Sync {
    /// Process a single event and update the graph
    async fn project(&self, event: &EventEnvelope) -> Result<()>;

    /// Execute a SPARQL query against the cluster graph
    async fn query(&self, sparql: &str) -> Result<QueryResult>;

    /// Execute a SPARQL query against a product's named graph
    async fn query_product(&self, product_iri: &ResourceIri, sparql: &str)
        -> Result<QueryResult>;
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub bindings: Vec<serde_json::Value>,
}

// ---- Identity ----

/// Issue and validate identity tokens. Manage passkeys and workload certs.
/// Implemented by: picloud-iam
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    async fn issue_token(
        &self,
        identity_iri: &ResourceIri,
        product: Option<&str>,
    ) -> Result<String>;

    async fn validate_token(&self, token: &str) -> Result<ValidatedIdentity>;

    async fn issue_workload_certificate(
        &self,
        workload_iri: &ResourceIri,
    ) -> Result<WorkloadCertificate>;
}

#[derive(Debug, Clone)]
pub struct ValidatedIdentity {
    pub identity_iri: ResourceIri,
    pub product: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkloadCertificate {
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

// ---- Storage ----

/// Allocate and manage block volumes across the cluster.
/// Implemented by: picloud-storage
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn allocate_volume(
        &self,
        volume_iri: &ResourceIri,
        size_gb: u64,
        intent: &crate::storage::StorageIntent,
    ) -> Result<VolumeHandle>;

    async fn delete_volume(&self, volume_iri: &ResourceIri) -> Result<()>;

    async fn available_capacity_gb(&self) -> Result<u64>;
}

#[derive(Debug, Clone)]
pub struct VolumeHandle {
    pub volume_iri: ResourceIri,
    pub device_path: String,
    pub replicated_to: Vec<Uuid>,
}

// ---- Workload Scheduler ----

/// Schedule and manage container and binary workloads.
/// Implemented by: picloud-workload
#[async_trait]
pub trait WorkloadScheduler: Send + Sync {
    async fn schedule(
        &self,
        workload_iri: &ResourceIri,
        spec: &WorkloadSpec,
    ) -> Result<WorkloadHandle>;

    async fn stop(&self, workload_iri: &ResourceIri) -> Result<()>;

    async fn status(&self, workload_iri: &ResourceIri) -> Result<WorkloadStatus>;
}

#[derive(Debug, Clone)]
pub enum WorkloadSpec {
    Container(crate::workload::ContainerSpec),
    Binary(crate::workload::BinarySpec),
}

#[derive(Debug, Clone)]
pub struct WorkloadHandle {
    pub workload_iri: ResourceIri,
    pub node_id: Uuid,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum WorkloadStatus {
    Running,
    Stopped,
    Failed { reason: String },
    Unknown,
}

// ---- Cluster ----

/// Cluster membership and node management.
/// Implemented by: picloud-cluster
#[async_trait]
pub trait ClusterMembership: Send + Sync {
    async fn is_leader(&self) -> bool;
    async fn leader_id(&self) -> Result<Uuid>;
    async fn members(&self) -> Result<Vec<NodeInfo>>;
    async fn local_node_id(&self) -> Uuid;
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: Uuid,
    pub node_iri: ResourceIri,
    pub address: String,
    pub is_leader: bool,
}

// ---- DNS ----

/// Register and resolve internal DNS names.
/// Implemented by: picloud-network
#[async_trait]
pub trait DnsRegistry: Send + Sync {
    async fn register(&self, iri: &ResourceIri, address: &str) -> Result<()>;
    async fn deregister(&self, iri: &ResourceIri) -> Result<()>;
    async fn resolve(&self, iri: &ResourceIri) -> Result<String>;
}
