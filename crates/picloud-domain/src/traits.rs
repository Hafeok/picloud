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
use crate::events::{CertType, EventEnvelope, MetricRecord, SpanRecord, TelemetryFilter};
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
    /// Audience (product IRI) this token is intended for (ADR-051)
    pub audience: Option<String>,
    /// Scopes granted in this token (ADR-051)
    pub scopes: Vec<String>,
    /// Flattened permissions (ADR-051)
    pub permissions: Vec<String>,
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

// ---- Snapshot Manager (ADR-047) ----

/// Manage volume snapshots and backups.
/// Implemented by: picloud-storage
#[async_trait]
pub trait SnapshotManager: Send + Sync {
    /// Create a snapshot of a volume. Returns the path where it was stored.
    async fn create_snapshot(&self, volume_iri: &ResourceIri) -> Result<SnapshotInfo>;

    /// List all snapshots for a volume.
    async fn list_snapshots(&self, volume_iri: &ResourceIri) -> Result<Vec<SnapshotInfo>>;

    /// Restore a volume from a snapshot at the given timestamp.
    async fn restore_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_timestamp: &str,
    ) -> Result<()>;

    /// Delete a snapshot by path.
    async fn delete_snapshot(
        &self,
        volume_iri: &ResourceIri,
        snapshot_path: &str,
    ) -> Result<()>;

    /// List all offsite backups for a volume.
    async fn list_backups(&self, volume_iri: &ResourceIri) -> Result<Vec<BackupInfo>>;
}

/// Information about a volume snapshot (ADR-047).
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub volume_iri: ResourceIri,
    pub snapshot_path: String,
    pub size_bytes: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Information about an offsite backup (ADR-047).
#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub volume_iri: ResourceIri,
    pub backup_path: String,
    pub size_bytes: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
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

// ---- Certificate Authority (ADR-053) ----

/// A signed certificate returned by the cluster CA.
#[derive(Debug, Clone)]
pub struct SignedCert {
    pub cert_pem: String,
    pub expires_at: DateTime<Utc>,
    pub fingerprint: String,
}

/// Cluster certificate authority — issues and signs certificates for nodes,
/// workloads, and ingress hostnames.
/// Implemented by: picloud-network (ca module)
#[async_trait]
pub trait CertificateAuthority: Send + Sync {
    /// Sign a node's CSR and return the signed certificate.
    /// Validates: CN matches node_name, no wildcard SANs.
    async fn sign_node_csr(
        &self,
        csr_der: &[u8],
        node_name: &str,
        node_ip: &str,
    ) -> Result<SignedCert>;

    /// Issue a workload identity certificate with a URI SAN.
    async fn sign_workload_cert(
        &self,
        workload_iri: &ResourceIri,
        product: &str,
    ) -> Result<SignedCert>;

    /// Issue a TLS certificate for an ingress hostname.
    async fn sign_ingress_cert(&self, hostname: &str) -> Result<SignedCert>;

    /// Return the CA certificate in PEM format.
    fn ca_cert_pem(&self) -> &str;

    /// Return the certificate type's lifetime parameters.
    fn cert_lifetime(&self, cert_type: &CertType) -> std::time::Duration;
}

// ---- Authoritative DNS (ADR-052) ----

/// DNS record returned by the authoritative resolver.
#[derive(Debug, Clone)]
pub enum DnsRecord {
    /// IPv4 address record
    A { address: std::net::Ipv4Addr },
    /// Service discovery record
    Srv { target: String, port: u16, priority: u16, weight: u16 },
    /// Text metadata record
    Txt { text: String },
    /// Reverse DNS record
    Ptr { name: String },
}

/// Authoritative DNS server for the cluster's tenant domain.
/// Answers queries by projecting the RDF graph, with an in-memory cache.
/// Implemented by: picloud-network (dns module)
#[async_trait]
pub trait AuthoritativeDns: Send + Sync {
    /// Resolve a DNS query. Returns matching records or empty vec for NXDOMAIN.
    async fn resolve(&self, name: &str, record_type: &str) -> Result<Vec<DnsRecord>>;

    /// Check if this server is authoritative for the given domain.
    fn is_authoritative(&self, domain: &str) -> bool;

    /// Invalidate cached entries matching a hostname pattern.
    /// Called when events indicate DNS-relevant state has changed.
    async fn invalidate_cache(&self, hostname: &str) -> Result<()>;
}

// ---- Token Exchange (ADR-051) ----

/// Extend IdentityProvider with token exchange capabilities.
/// This is a separate trait to avoid breaking the existing IdentityProvider.
/// Implemented by: picloud-iam
#[async_trait]
pub trait TokenExchange: Send + Sync {
    /// RFC 8693 token exchange — issue a new token on behalf of the subject.
    /// Validates the subject token, checks permissions in the target product,
    /// resolves roles with OWL inheritance, and issues a scoped token.
    async fn token_exchange(
        &self,
        request: &crate::identity::TokenExchangeRequest,
    ) -> Result<crate::identity::TokenResponse>;

    /// Resolve all roles for an identity within a product, including
    /// inherited roles via rdfs:subClassOf traversal. Returns flattened
    /// permissions and merged custom claims.
    async fn resolve_roles(
        &self,
        identity_iri: &ResourceIri,
        product: &str,
    ) -> Result<crate::identity::ResolvedTokenClaims>;
}

// ---- OCI Registry (ADR-054) ----

/// Store and serve OCI container images via the Distribution API v2.
/// Implemented by: picloud-registry
#[async_trait]
pub trait RegistryBackend: Send + Sync {
    /// Store a content-addressed blob. Digest is verified on write.
    async fn put_blob(&self, digest: &str, data: Vec<u8>) -> Result<()>;

    /// Retrieve a blob by its content digest.
    async fn get_blob(&self, digest: &str) -> Result<Vec<u8>>;

    /// Check if a blob exists without reading it.
    async fn blob_exists(&self, digest: &str) -> Result<bool>;

    /// Delete a blob by digest (used by GC).
    async fn delete_blob(&self, digest: &str) -> Result<()>;

    /// Store a manifest (image config + layer references).
    /// Returns the digest of the stored manifest.
    async fn put_manifest(
        &self,
        repository: &str,
        reference: &str,
        content_type: &str,
        data: Vec<u8>,
    ) -> Result<String>;

    /// Retrieve a manifest by tag or digest. Returns (content_type, data).
    async fn get_manifest(
        &self,
        repository: &str,
        reference: &str,
    ) -> Result<(String, Vec<u8>)>;

    /// Delete a manifest by tag or digest.
    async fn delete_manifest(&self, repository: &str, reference: &str) -> Result<()>;

    /// List all tags in a repository.
    async fn list_tags(&self, repository: &str) -> Result<Vec<String>>;

    /// List all repositories.
    async fn list_repositories(&self) -> Result<Vec<String>>;

    /// Start a blob upload session. Returns an upload UUID.
    async fn start_blob_upload(&self, repository: &str) -> Result<String>;

    /// Run garbage collection — delete unreferenced blobs.
    async fn garbage_collect(&self) -> Result<GarbageCollectionResult>;

    /// Get registry storage statistics.
    async fn stats(&self) -> Result<RegistryStats>;
}

/// Result of a garbage collection run.
#[derive(Debug, Clone)]
pub struct GarbageCollectionResult {
    pub blobs_scanned: u64,
    pub blobs_deleted: u64,
    pub bytes_freed: u64,
    pub duration_ms: u64,
}

/// Registry storage statistics.
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total_blobs: u64,
    pub total_repositories: u64,
    pub storage_used_bytes: u64,
}
