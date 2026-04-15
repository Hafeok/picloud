use thiserror::Error;
use uuid::Uuid;

/// The single error type for all domain operations.
/// Each slice defines its own error variant here so callers
/// can match exhaustively without importing slice internals.
#[derive(Debug, Error)]
pub enum PiCloudError {
    // --- Resource errors ---
    #[error("Resource not found: {iri}")]
    ResourceNotFound { iri: String },

    #[error("Resource already exists: {iri}")]
    ResourceAlreadyExists { iri: String },

    #[error("Resource validation failed: {reason}")]
    ResourceValidationFailed { reason: String },

    // --- Event errors ---
    #[error("Event schema not found: {schema_iri}")]
    EventSchemaNotFound { schema_iri: String },

    #[error("Event append failed for aggregate {aggregate_id}: {reason}")]
    EventAppendFailed { aggregate_id: Uuid, reason: String },

    // --- IAM errors ---
    #[error("Unauthenticated — no valid identity token")]
    Unauthenticated,

    #[error("Unauthorized — identity {identity} lacks permission {permission}")]
    Unauthorized { identity: String, permission: String },

    #[error("Passkey challenge failed: {reason}")]
    PasskeyChallengeFailed { reason: String },

    // --- Storage errors ---
    #[error("Volume not found: {volume_id}")]
    VolumeNotFound { volume_id: Uuid },

    #[error("Insufficient storage capacity: requested {requested_gb}GB, available {available_gb}GB")]
    InsufficientCapacity { requested_gb: u64, available_gb: u64 },

    #[error("Storage operation failed: {0}")]
    Storage(String),

    // --- Cluster errors ---
    #[error("No cluster leader available")]
    NoLeader,

    #[error("Node {node_id} is not a cluster member")]
    UnknownNode { node_id: Uuid },

    // --- Network errors ---
    #[error("TLS certificate error: {reason}")]
    TlsCertificateError { reason: String },

    // --- IRI errors ---
    #[error("Invalid IRI: {iri}")]
    InvalidIri { iri: String },

    // --- Secret errors ---
    #[error("Secret not found: {product}/{name}")]
    SecretNotFound { product: String, name: String },

    // --- Workload errors ---
    #[error("Workload {name} failed to start: {reason}")]
    WorkloadStartFailed { name: String, reason: String },

    #[error("Invalid resource limits: {reason}")]
    InvalidResourceLimits { reason: String },

    // --- Cluster identity errors (ADR-042) ---
    #[error("Node join rejected: {reason}")]
    NodeJoinRejected { reason: String },

    #[error("Cluster already initialized (cluster_id: {cluster_id})")]
    ClusterAlreadyInitialized { cluster_id: String },

    // --- Telemetry errors (ADR-045, ADR-046) ---
    #[error("Telemetry write failed: {reason}")]
    TelemetryWriteFailed { reason: String },

    #[error("Telemetry query failed: {reason}")]
    TelemetryQueryFailed { reason: String },

    #[error("SQL parse error: {reason}")]
    SqlParseError { reason: String },

    // --- DNS errors (ADR-052) ---
    #[error("DNS resolution failed for {hostname}")]
    DnsResolutionFailed { hostname: String },

    // --- Certificate enrollment errors (ADR-053) ---
    #[error("Node enrollment failed: {reason}")]
    EnrollmentFailed { reason: String },

    #[error("Certificate revoked: {fingerprint}")]
    CertificateRevoked { fingerprint: String },

    // --- IAM audience errors (ADR-051) ---
    #[error("Audience mismatch: expected {expected}, got {got}")]
    AudienceMismatch { expected: String, got: String },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    // --- Registry errors (ADR-054) ---
    #[error("Image not found: {repository}:{reference}")]
    ImageNotFound { repository: String, reference: String },

    #[error("Blob not found: {digest}")]
    BlobNotFound { digest: String },

    #[error("Registry error: {reason}")]
    RegistryError { reason: String },

    // --- Capability errors (ADR-055) ---
    #[error("Capability not found: {name}")]
    CapabilityNotFound { name: String },

    #[error("Capability unfulfilled: no implementor for '{capability}' at version >= {min_version}")]
    CapabilityUnfulfilled { capability: String, min_version: String },

    #[error("Capability deletion blocked: {consumers} consumer(s) depend on '{capability}'")]
    CapabilityDeletionBlocked { capability: String, consumers: usize },

    #[error("Capability SHACL conformance failed for product '{product}' implementing '{capability}': {reason}")]
    CapabilityShaclConformanceFailed { product: String, capability: String, reason: String },

    #[error("Capability routing failed for '{capability}': {reason}")]
    CapabilityRoutingFailed { capability: String, reason: String },

    // --- Event routing errors (ADR-022, FT-084) ---
    #[error("Event routing failed for subscription '{subscription}': {reason}")]
    EventRoutingFailed { subscription: String, reason: String },

    // --- Data Product / Domain errors (ADR-056) ---
    #[error("Data domain not found: {name}")]
    DataDomainNotFound { name: String },

    #[error("Data domain deletion blocked: {members} data product(s) still assigned to '{domain}'")]
    DataDomainDeletionBlocked { domain: String, members: usize },

    #[error("Data product not found: {name}")]
    DataProductNotFound { name: String },

    #[error("Data product deletion blocked: {consumers} consumer(s) depend on '{data_product}'")]
    DataProductDeletionBlocked { data_product: String, consumers: usize },

    #[error("Cross-product graph access denied: {requester} cannot access {target}'s internal graph")]
    CrossProductGraphAccessDenied { requester: String, target: String },

    // --- Generic ---
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, PiCloudError>;
