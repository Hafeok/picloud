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

    // --- CSR validation errors (ADR-055) ---
    #[error("CSR validation failed: {reason}")]
    CsrValidationFailed { reason: String },

    // --- ACME errors (ADR-055) ---
    #[error("ACME protocol error: {reason}")]
    AcmeProtocolError { reason: String },

    // --- BYO-CA errors (ADR-030, ADR-055) ---
    #[error("External CA error: {reason}")]
    ExternalCaError { reason: String },

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

    // --- Generic ---
    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, PiCloudError>;
