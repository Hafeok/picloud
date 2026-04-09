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

// ---------------------------------------------------------------------------
// WebAuthn / passkey ceremony types
// ---------------------------------------------------------------------------

/// Unique identifier for an in-flight challenge (registration or authentication).
pub type ChallengeId = String;

/// Options sent to the client to begin a WebAuthn registration ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationChallenge {
    /// The challenge the client must sign with the new credential.
    pub challenge: String,
    /// The relying party ID (domain).
    pub rp_id: String,
    /// The relying party display name.
    pub rp_name: String,
    /// The user identifier (opaque bytes, base64-encoded).
    pub user_id: String,
    /// The user display name.
    pub user_name: String,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
}

/// The client's response to a registration challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationResponse {
    /// The new credential ID (base64url-encoded).
    pub credential_id: String,
    /// The public key in COSE format (base64url-encoded).
    pub public_key: String,
    /// The attestation object (base64url-encoded) — simplified, not fully verified.
    pub attestation: Option<String>,
    /// Optional authenticator AAGUID.
    pub aaguid: Option<String>,
    /// Display name the user gave this authenticator.
    pub display_name: Option<String>,
}

/// Options sent to the client to begin a WebAuthn authentication ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationChallenge {
    /// The challenge bytes (base64url-encoded).
    pub challenge: String,
    /// The relying party ID.
    pub rp_id: String,
    /// The credential IDs the client may use (base64url-encoded).
    pub allow_credentials: Vec<String>,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
}

/// The client's response to an authentication challenge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationResponse {
    /// The credential ID that was used (base64url-encoded).
    pub credential_id: String,
    /// The signed challenge data (base64url-encoded).
    pub signature: String,
    /// The authenticator data (base64url-encoded).
    pub authenticator_data: Option<String>,
    /// The client data JSON (base64url-encoded).
    pub client_data_json: Option<String>,
    /// Signature format: "webauthn" for real ECDSA from a FIDO2 authenticator,
    /// "hmac" for the simplified HMAC-based flow (default for backward compat).
    #[serde(default = "default_signature_format")]
    pub signature_format: String,
}

fn default_signature_format() -> String {
    "hmac".to_string()
}

/// Device flow — the CLI requests a device code and polls for completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFlowResponse {
    /// The device code the CLI uses to poll.
    pub device_code: String,
    /// The URL the user opens in a browser.
    pub verification_url: String,
    /// The interval (in seconds) the CLI should wait between polls.
    pub interval_secs: u64,
    /// When this device code expires.
    pub expires_in_secs: u64,
}

/// Result of polling a device flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum DeviceFlowPollResult {
    /// The user has not yet completed authentication.
    #[serde(rename = "pending")]
    Pending,
    /// Authentication is complete; here is the token.
    #[serde(rename = "complete")]
    Complete { access_token: String, token_type: String, expires_in: i64 },
    /// The device code has expired.
    #[serde(rename = "expired")]
    Expired,
}

/// An RBAC role — additive permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub product: Option<String>,
    pub permissions: Vec<Permission>,
    /// Parent role name — resolved transitively via OWL inference (rdfs:subClassOf) (ADR-051)
    #[serde(default)]
    pub inherits: Option<String>,
    /// Static key-value claims added to tokens for users with this role (ADR-051)
    #[serde(default)]
    pub claims: std::collections::HashMap<String, String>,
}

/// A product-scoped OAuth scope (ADR-051)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductScope {
    pub name: String,
    pub product: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Permission patterns granted when this scope is requested
    pub permissions: Vec<String>,
    /// Static claims added when this scope is granted
    #[serde(default)]
    pub claims: std::collections::HashMap<String, String>,
}

/// M2M permission — declares which products may request client_credentials
/// tokens against this product (ADR-051)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct M2mPermission {
    pub name: String,
    /// The product granting access (the resource owner)
    pub product: String,
    /// The product being granted access (the client)
    pub client: String,
    pub scopes: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// The resolved claims for a token — assembled at issuance time
/// from roles (with inherited permissions), scopes, and custom claims (ADR-051)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTokenClaims {
    pub subject: String,
    pub audience: String,
    pub issuer: String,
    pub scopes: Vec<String>,
    pub roles: Vec<String>,
    /// Full permission set — flattened from all assigned roles (including inherited)
    pub permissions: Vec<String>,
    /// Custom claims — role claims merged with scope claims, role wins on conflict
    pub custom: std::collections::HashMap<String, String>,
    /// Actor claim — present in on-behalf-of tokens (RFC 8693)
    #[serde(default)]
    pub actor: Option<String>,
}

/// Token flow type — determines how the token was issued (ADR-051)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenFlow {
    /// Standard OIDC user authentication
    UserAuth,
    /// RFC 8693 on-behalf-of — user delegated to a product
    OnBehalfOf { actor_product: String },
    /// OAuth 2.0 client credentials — M2M
    ClientCredentials { client_product: String },
}

/// Token exchange request (RFC 8693) (ADR-051)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenExchangeRequest {
    pub grant_type: String,
    /// The incoming access token
    pub subject_token: String,
    pub subject_token_type: String,
    /// Target audience (product IRI)
    #[serde(default)]
    pub audience: Option<String>,
    /// Requested scopes
    #[serde(default)]
    pub scope: Option<String>,
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

    #[test]
    fn role_with_inheritance_serde() {
        let role = Role {
            name: "editor".to_string(),
            product: Some("photo-app".to_string()),
            permissions: vec![Permission {
                resource_pattern: "https://picloud.local/products/photo-app/*".to_string(),
                action: PermissionAction::Write,
            }],
            inherits: Some("viewer".to_string()),
            claims: std::collections::HashMap::from([
                ("tier".to_string(), "premium".to_string()),
            ]),
        };
        let json = serde_json::to_value(&role).unwrap();
        assert_eq!(json["inherits"], "viewer");
        assert_eq!(json["claims"]["tier"], "premium");
        let back: Role = serde_json::from_value(json).unwrap();
        assert_eq!(back.inherits, Some("viewer".to_string()));
        assert_eq!(back.claims.get("tier"), Some(&"premium".to_string()));
    }

    #[test]
    fn product_scope_serde() {
        let scope = ProductScope {
            name: "photos:read".to_string(),
            product: "photo-app".to_string(),
            description: Some("Read access to photos".to_string()),
            permissions: vec!["photos.read".to_string()],
            claims: std::collections::HashMap::from([
                ("access_level".to_string(), "read".to_string()),
            ]),
        };
        let json = serde_json::to_value(&scope).unwrap();
        assert_eq!(json["name"], "photos:read");
        let back: ProductScope = serde_json::from_value(json).unwrap();
        assert_eq!(back.permissions, vec!["photos.read"]);
    }

    #[test]
    fn m2m_permission_serde() {
        let perm = M2mPermission {
            name: "billing-to-photos".to_string(),
            product: "photo-app".to_string(),
            client: "billing-app".to_string(),
            scopes: vec!["photos:read".to_string()],
            description: Some("Billing reads photo metadata".to_string()),
        };
        let json = serde_json::to_value(&perm).unwrap();
        assert_eq!(json["client"], "billing-app");
        let back: M2mPermission = serde_json::from_value(json).unwrap();
        assert_eq!(back.scopes, vec!["photos:read"]);
    }

    #[test]
    fn resolved_token_claims_serde() {
        let claims = ResolvedTokenClaims {
            subject: "https://picloud.local/identities/alice".to_string(),
            audience: "https://picloud.local/products/photo-app".to_string(),
            issuer: "https://picloud.local".to_string(),
            scopes: vec!["photos:read".to_string()],
            roles: vec!["editor".to_string()],
            permissions: vec!["photos.read".to_string(), "photos.write".to_string()],
            custom: std::collections::HashMap::from([
                ("tier".to_string(), "premium".to_string()),
            ]),
            actor: None,
        };
        let json = serde_json::to_value(&claims).unwrap();
        assert_eq!(json["audience"], "https://picloud.local/products/photo-app");
        let back: ResolvedTokenClaims = serde_json::from_value(json).unwrap();
        assert_eq!(back.permissions.len(), 2);
    }

    #[test]
    fn token_exchange_request_serde() {
        let req = TokenExchangeRequest {
            grant_type: "urn:ietf:params:oauth:grant-type:token-exchange".to_string(),
            subject_token: "eyJhbGc...".to_string(),
            subject_token_type: "urn:ietf:params:oauth:token-type:access_token".to_string(),
            audience: Some("https://picloud.local/products/photo-app".to_string()),
            scope: Some("photos:read".to_string()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["grant_type"], "urn:ietf:params:oauth:grant-type:token-exchange");
        let back: TokenExchangeRequest = serde_json::from_value(json).unwrap();
        assert_eq!(back.audience, Some("https://picloud.local/products/photo-app".to_string()));
    }

    #[test]
    fn token_flow_serde() {
        let flow = TokenFlow::OnBehalfOf {
            actor_product: "https://picloud.local/products/billing".to_string(),
        };
        let json = serde_json::to_value(&flow).unwrap();
        let back: TokenFlow = serde_json::from_value(json).unwrap();
        match back {
            TokenFlow::OnBehalfOf { actor_product } => {
                assert!(actor_product.contains("billing"));
            }
            _ => panic!("expected OnBehalfOf"),
        }
    }
}
