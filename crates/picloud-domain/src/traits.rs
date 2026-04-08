/// Domain Traits
///
/// These are the abstractions that slices implement.
/// Slices depend only on these traits, never on each other's concrete types.
/// The composition root (picloud binary) wires implementations together.
///
/// This is how vertical slice architecture is enforced at the type level.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use crate::error::Result;
use crate::events::{EventEnvelope, MetricRecord, SpanRecord, TelemetryFilter};
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

    /// Return all events starting from the given offset (0-based index).
    ///
    /// This is the catchup mechanism: a projector that has processed events
    /// 0..N calls `events_since(N)` to get everything it missed. A new node
    /// joining the cluster calls `events_since(0)` to replay the full log.
    async fn events_since(&self, offset: usize) -> Vec<EventEnvelope>;
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

    /// Return the OIDC discovery document for this provider.
    async fn oidc_discovery(&self) -> Result<crate::identity::OidcDiscoveryDocument>;

    /// Return the JSON Web Key Set for token verification.
    async fn jwks(&self) -> Result<crate::identity::JsonWebKeySet>;

    /// Authenticate using client credentials (app registration) and return a token response.
    async fn client_credentials_token(
        &self,
        client_id: &str,
        client_secret: &str,
        scope: Option<&str>,
    ) -> Result<crate::identity::TokenResponse>;

    /// Register an app (OIDC client) for a product. Returns the client_id and plaintext secret.
    async fn register_app(
        &self,
        product_iri: &ResourceIri,
        redirect_uris: Vec<String>,
        scopes: Vec<String>,
    ) -> Result<crate::identity::AppRegistration>;

    // ---- WebAuthn / passkey ceremonies ----

    /// Begin a passkey registration ceremony for a human identity.
    /// Returns a challenge ID (to correlate begin/complete) and the options for the client.
    async fn begin_registration(
        &self,
        identity_iri: &ResourceIri,
    ) -> Result<(crate::identity::ChallengeId, crate::identity::RegistrationChallenge)>;

    /// Complete a passkey registration ceremony.
    /// Validates the challenge and stores the new credential.
    async fn complete_registration(
        &self,
        challenge_id: &str,
        response: crate::identity::RegistrationResponse,
    ) -> Result<crate::identity::RegisteredPasskey>;

    /// Begin a passkey authentication ceremony for a human identity.
    /// Returns a challenge ID and the options (including allowed credential IDs).
    async fn begin_authentication(
        &self,
        identity_iri: &ResourceIri,
    ) -> Result<(crate::identity::ChallengeId, crate::identity::AuthenticationChallenge)>;

    /// Complete a passkey authentication ceremony.
    /// Verifies the signed challenge, returns an access token on success.
    async fn complete_authentication(
        &self,
        challenge_id: &str,
        response: crate::identity::AuthenticationResponse,
    ) -> Result<String>;

    /// Exchange an enrollment token (bootstrap/recovery) for a registration challenge.
    /// The token is validated and marked as used.
    async fn enroll_with_token(
        &self,
        token: &str,
    ) -> Result<(crate::identity::ChallengeId, crate::identity::RegistrationChallenge)>;

    /// Begin a device flow — returns a device code and verification URL.
    async fn begin_device_flow(&self) -> Result<crate::identity::DeviceFlowResponse>;

    /// Poll a device flow — returns pending, complete (with token), or expired.
    async fn poll_device_flow(
        &self,
        device_code: &str,
    ) -> Result<crate::identity::DeviceFlowPollResult>;

    /// Mark a device flow as authenticated (called after the user completes passkey auth in the browser).
    async fn complete_device_flow(
        &self,
        device_code: &str,
        identity_iri: &ResourceIri,
    ) -> Result<()>;
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

// ---- Secrets ----

/// Encrypt, store, retrieve, and delete secrets.
/// Implemented by: picloud-iam
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Encrypt and store a secret value for a product.
    async fn store_secret(&self, product: &str, name: &str, value: &str) -> Result<()>;

    /// Decrypt and return a secret value.
    async fn get_secret(&self, product: &str, name: &str) -> Result<String>;

    /// Delete a stored secret.
    async fn delete_secret(&self, product: &str, name: &str) -> Result<()>;
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

// ---- Alert Engine (ADR-041) ----

/// Evaluate built-in alert rules against recorded metrics and emit
/// AlertFired / AlertResolved events. Tracks dampening state.
/// Implemented by: picloud-http (or wherever the inference engine lives)
#[async_trait]
pub trait AlertEvaluator: Send + Sync {
    /// Evaluate all registered alert rules against current metric state.
    /// Returns the list of alert events (fired or resolved) produced.
    async fn evaluate(
        &self,
        node_iri: &ResourceIri,
        metrics: &[crate::events::MetricEntry],
    ) -> Result<Vec<AlertAction>>;
}

/// An action produced by the alert evaluator.
#[derive(Debug, Clone)]
pub enum AlertAction {
    Fire(crate::events::AlertFiredPayload),
    Resolve(crate::events::AlertResolvedPayload),
}

// ---- Cluster Identity (ADR-042) ----

/// Manage the cluster's immutable identity.
/// Implemented by: picloud-cluster
#[async_trait]
pub trait ClusterIdentityStore: Send + Sync {
    /// Initialize the cluster identity. Fails if already initialized.
    async fn initialize(
        &self,
        identity: crate::resources::ClusterIdentity,
    ) -> Result<()>;

    /// Retrieve the cluster identity, if initialized.
    async fn get(&self) -> Result<Option<crate::resources::ClusterIdentity>>;

    /// Validate a node join request. Returns Ok(()) if valid, Err with reason if not.
    async fn validate_node_join(
        &self,
        node_id: Uuid,
        ca_fingerprint: &str,
        cluster_id: Uuid,
    ) -> Result<()>;
}

// ---- Event Replay (ADR-035) ----

/// Request parameters for a replay operation.
#[derive(Debug, Clone)]
pub struct ReplayRequest {
    /// Start of the replay time range.
    pub from: DateTime<Utc>,
    /// End of the replay time range (None = replay to present).
    pub to: Option<DateTime<Utc>>,
    /// Product scope (None for platform-level replay).
    pub product: Option<String>,
    /// Optional aggregate type filter.
    pub aggregate_type: Option<String>,
    /// Optional aggregate ID filter.
    pub aggregate_ids: Vec<String>,
}

/// Result of a completed replay operation.
#[derive(Debug, Clone)]
pub struct ReplayResult {
    /// The replay operation ID.
    pub replay_id: Uuid,
    /// Number of events replayed.
    pub events_replayed: usize,
    /// IRI of the shadow graph that was used.
    pub shadow_graph_iri: String,
}

/// Replay engine — re-projects historical events through current projectors
/// into a shadow graph, then atomically swaps with the live graph.
/// Implemented by: picloud-rdf
#[async_trait]
pub trait ReplayEngine: Send + Sync {
    /// Execute a replay operation.
    /// Returns a replay_id immediately; progress and completion
    /// are reported via ReplayStarted/Progress/Completed/Failed events.
    async fn start_replay(&self, request: ReplayRequest) -> Result<Uuid>;
}

// ---- Telemetry Store (ADR-046) ----

/// Store and query telemetry data (spans, metrics).
/// The interface is designed for a future Parquet/DataFusion backend,
/// but the initial implementation uses JSON-lines files with hourly partitioning.
/// Implemented by: picloud-http (OtelTelemetryStore)
#[async_trait]
pub trait TelemetryStore: Send + Sync {
    /// Write a batch of span records to the store.
    async fn write_spans(&self, spans: Vec<SpanRecord>) -> Result<()>;

    /// Write a batch of metric records to the store.
    async fn write_metrics(&self, metrics: Vec<MetricRecord>) -> Result<()>;

    /// Query span records within a time range, optionally filtered.
    async fn query_spans(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        filter: TelemetryFilter,
    ) -> Result<Vec<SpanRecord>>;

    /// Query metric records within a time range, optionally filtered.
    async fn query_metrics(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        filter: TelemetryFilter,
    ) -> Result<Vec<MetricRecord>>;
}
