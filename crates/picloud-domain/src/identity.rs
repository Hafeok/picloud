/// Identity Types
///
/// PiCloud is a full OIDC provider (ADR-017).
/// Human identities use passkeys/FIDO2 only — no passwords (ADR-025).
/// Workload identities use mTLS certificates injected by the platform.
/// Products act as OIDC App Registrations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::iri::ResourceIri;

/// A human identity — authenticated via passkey/FIDO2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanIdentity {
    pub id: Uuid,
    pub iri: ResourceIri,
    pub name: String,
    pub email: Option<String>,
    pub passkeys: Vec<RegisteredPasskey>,
    pub platform_roles: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl HumanIdentity {
    /// Enforces ADR-026: admin accounts must have >= 2 passkeys
    pub fn can_remove_passkey(&self, is_admin: bool) -> bool {
        if is_admin {
            self.passkeys.len() > 2
        } else {
            self.passkeys.len() > 1
        }
    }
}

/// A registered WebAuthn/FIDO2 credential bound to a human identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredPasskey {
    pub credential_id: String,
    pub public_key: Vec<u8>,
    pub aaguid: Option<String>,
    pub registered_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub display_name: Option<String>,
}

/// A workload identity — mTLS certificate injected at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadIdentity {
    pub id: Uuid,
    pub iri: ResourceIri,
    pub name: String,
    pub product: String,
    pub roles: Vec<String>,
    pub certificate_fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A Product App Registration — OIDC client for applications (ADR-017)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRegistration {
    pub id: Uuid,
    pub client_id: String,
    /// Hashed — never stored in plaintext
    pub client_secret_hash: String,
    pub product_iri: ResourceIri,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
}

/// A bootstrap or re-enrollment token (ADR-026)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub token: String,
    pub purpose: EnrollmentPurpose,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
    /// For admin resets — which identity is being re-enrolled
    pub target_identity: Option<ResourceIri>,
}

impl EnrollmentToken {
    pub fn is_valid(&self) -> bool {
        !self.used && Utc::now() < self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentPurpose {
    /// First admin on a fresh cluster
    Bootstrap,
    /// Admin-initiated passkey reset for a user
    PasskeyReset,
    /// Physical recovery — operator ran `picloud cluster recover`
    PhysicalRecovery,
}

/// An RBAC role — additive permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub product: Option<String>,
    pub permissions: Vec<Permission>,
}

/// A permission — scoped to a resource IRI pattern and an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    /// IRI pattern — may include wildcards
    /// e.g. "https://picloud.local/products/photo-app/*"
    pub resource_pattern: String,
    pub action: PermissionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    Read,
    Write,
    Delete,
    Execute,
    Query,  // SPARQL query
    Append, // event log append
}
