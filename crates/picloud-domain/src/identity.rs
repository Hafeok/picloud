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
    /// Groups this identity belongs to (explicit + inferred via rules) (ADR-036)
    #[serde(default)]
    pub groups: Vec<ResourceIri>,
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

/// OIDC discovery document — returned at /.well-known/openid-configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcDiscoveryDocument {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub response_types_supported: Vec<String>,
    pub subject_types_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
}

/// A JSON Web Key — part of the JWKS response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonWebKey {
    pub kty: String,
    pub kid: String,
    pub alg: String,
    #[serde(rename = "use")]
    pub key_use: String,
    /// For HMAC keys, this is "HS256" and the key value is not exposed.
    /// For RSA/EC keys, include the public key components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
}

/// JSON Web Key Set — returned at /.well-known/jwks.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonWebKeySet {
    pub keys: Vec<JsonWebKey>,
}

/// Token response from the /auth/token endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Token request for client_credentials grant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCredentialsRequest {
    pub grant_type: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iri::ResourceIri;
    use chrono::Utc;

    fn make_passkey() -> RegisteredPasskey {
        RegisteredPasskey {
            credential_id: "cred-1".to_string(),
            public_key: vec![1, 2, 3],
            aaguid: None,
            registered_at: Utc::now(),
            last_used_at: None,
            display_name: None,
        }
    }

    fn make_identity(num_passkeys: usize) -> HumanIdentity {
        let now = Utc::now();
        HumanIdentity {
            id: Uuid::new_v4(),
            iri: ResourceIri("https://picloud.local/identities/test".to_string()),
            name: "Test User".to_string(),
            email: None,
            passkeys: (0..num_passkeys).map(|_| make_passkey()).collect(),
            platform_roles: vec![],
            groups: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn admin_with_3_passkeys_can_remove() {
        let identity = make_identity(3);
        assert!(identity.can_remove_passkey(true));
    }

    #[test]
    fn admin_with_2_passkeys_cannot_remove() {
        let identity = make_identity(2);
        assert!(!identity.can_remove_passkey(true));
    }

    #[test]
    fn admin_with_1_passkey_cannot_remove() {
        let identity = make_identity(1);
        assert!(!identity.can_remove_passkey(true));
    }

    #[test]
    fn non_admin_with_2_passkeys_can_remove() {
        let identity = make_identity(2);
        assert!(identity.can_remove_passkey(false));
    }

    #[test]
    fn non_admin_with_1_passkey_cannot_remove() {
        let identity = make_identity(1);
        assert!(!identity.can_remove_passkey(false));
    }

    #[test]
    fn enrollment_token_valid_when_not_used_and_not_expired() {
        let token = EnrollmentToken {
            token: "tok-123".to_string(),
            purpose: EnrollmentPurpose::Bootstrap,
            expires_at: Utc::now() + chrono::Duration::hours(1),
            used: false,
            target_identity: None,
        };
        assert!(token.is_valid());
    }

    #[test]
    fn enrollment_token_invalid_when_used() {
        let token = EnrollmentToken {
            token: "tok-123".to_string(),
            purpose: EnrollmentPurpose::Bootstrap,
            expires_at: Utc::now() + chrono::Duration::hours(1),
            used: true,
            target_identity: None,
        };
        assert!(!token.is_valid());
    }

    #[test]
    fn enrollment_token_invalid_when_expired() {
        let token = EnrollmentToken {
            token: "tok-123".to_string(),
            purpose: EnrollmentPurpose::PasskeyReset,
            expires_at: Utc::now() - chrono::Duration::hours(1),
            used: false,
            target_identity: None,
        };
        assert!(!token.is_valid());
    }
}
