//! LocalIdentityProvider — implements IdentityProvider from picloud-domain.
//!
//! Issues HMAC-signed tokens and self-signed workload certificates.
//! Depends only on picloud-domain — never on other slices.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, Utc};
use ring::hmac;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::identity::{
    AppRegistration, AuthenticationChallenge, AuthenticationResponse, ChallengeId,
    DeviceFlowPollResult, DeviceFlowResponse, JsonWebKey, JsonWebKeySet, OidcDiscoveryDocument,
    RegisteredPasskey, RegistrationChallenge, RegistrationResponse, TokenResponse,
};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{IdentityProvider, QueryResult, ValidatedIdentity, WorkloadCertificate};

/// A function that runs a SPARQL query against the cluster graph.
/// Injected from the composition root to avoid cross-slice imports.
pub type SparqlQueryFn = Arc<
    dyn Fn(String) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<QueryResult>> + Send>,
    > + Send + Sync,
>;

/// Internal token claims — serialized to JSON then base64-encoded.
/// Extended with audience, scopes, and permissions for ADR-051 Product IAM.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenClaims {
    identity_iri: String,
    product: Option<String>,
    roles: Vec<String>,
    issued_at: i64,
    expires_at: i64,
    /// Audience — product IRI the token is intended for (ADR-051)
    #[serde(default)]
    aud: Option<String>,
    /// Scopes granted in this token (ADR-051)
    #[serde(default)]
    scopes: Vec<String>,
    /// Flattened permissions from roles (ADR-051)
    #[serde(default)]
    permissions: Vec<String>,
    /// Actor identity — present in on-behalf-of tokens (RFC 8693 / ADR-051)
    #[serde(default)]
    actor: Option<String>,
}

/// A stored identity with its associated roles.
#[derive(Debug, Clone)]
pub struct StoredIdentity {
    pub iri: ResourceIri,
    pub roles: Vec<String>,
}

/// A stored app registration for client credentials auth.
#[derive(Debug, Clone)]
pub struct StoredAppRegistration {
    pub registration: AppRegistration,
    /// Plaintext secret is only available at creation time — we store the hash.
    /// For validation we hash the incoming secret and compare.
    pub product_name: String,
}

/// An in-flight WebAuthn challenge awaiting completion.
#[derive(Debug, Clone)]
struct PendingChallenge {
    /// The identity this challenge belongs to.
    identity_iri: ResourceIri,
    /// The raw challenge bytes (base64url-encoded in the wire format).
    challenge_bytes: Vec<u8>,
    /// Whether this is a registration or authentication challenge.
    kind: ChallengeKind,
    /// When this challenge expires.
    expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
enum ChallengeKind {
    Registration,
    Authentication,
}

/// An in-flight device flow awaiting browser authentication.
#[derive(Debug, Clone)]
struct PendingDeviceFlow {
    /// When this device code expires.
    expires_at: chrono::DateTime<chrono::Utc>,
    /// Once the user authenticates in the browser, the token is stored here.
    completed_token: Option<String>,
    /// The identity that authenticated (set on completion).
    identity_iri: Option<ResourceIri>,
}

/// A stored enrollment token.
#[derive(Debug, Clone)]
struct StoredEnrollmentToken {
    token: picloud_domain::identity::EnrollmentToken,
}

/// Local identity provider backed by HMAC-SHA256 token signing and rcgen certificates.
pub struct LocalIdentityProvider {
    signing_key: hmac::Key,
    key_id: String,
    identities: Arc<RwLock<HashMap<String, StoredIdentity>>>,
    app_registrations: Arc<RwLock<HashMap<String, StoredAppRegistration>>>,
    /// Passkey credentials keyed by identity IRI.
    passkeys: Arc<RwLock<HashMap<String, Vec<RegisteredPasskey>>>>,
    /// In-flight WebAuthn challenges keyed by challenge ID.
    pending_challenges: Arc<RwLock<HashMap<String, PendingChallenge>>>,
    /// In-flight device flows keyed by device code.
    pending_device_flows: Arc<RwLock<HashMap<String, PendingDeviceFlow>>>,
    /// Enrollment tokens keyed by token string.
    enrollment_tokens: Arc<RwLock<HashMap<String, StoredEnrollmentToken>>>,
    _iri_builder: IriBuilder,
    cluster_domain: ClusterDomain,
    /// Token validity duration in seconds.
    token_ttl_secs: i64,
    /// Challenge validity duration in seconds.
    challenge_ttl_secs: i64,
    /// Optional SPARQL query function for OWL role inheritance (ADR-051).
    /// Injected from composition root when RDF store is available.
    sparql_query: Option<SparqlQueryFn>,
}

impl LocalIdentityProvider {
    /// Create a new provider with the given HMAC key material.
    pub fn new(key_material: &[u8], domain: ClusterDomain) -> Self {
        Self {
            signing_key: hmac::Key::new(hmac::HMAC_SHA256, key_material),
            key_id: "picloud-hmac-1".to_string(),
            identities: Arc::new(RwLock::new(HashMap::new())),
            app_registrations: Arc::new(RwLock::new(HashMap::new())),
            passkeys: Arc::new(RwLock::new(HashMap::new())),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            pending_device_flows: Arc::new(RwLock::new(HashMap::new())),
            enrollment_tokens: Arc::new(RwLock::new(HashMap::new())),
            _iri_builder: IriBuilder::new(domain.clone()),
            cluster_domain: domain,
            token_ttl_secs: 3600,
            challenge_ttl_secs: 300, // 5 minutes
            sparql_query: None,
        }
    }

    /// Create a provider with a custom token TTL (useful for testing).
    pub fn with_ttl(key_material: &[u8], domain: ClusterDomain, token_ttl_secs: i64) -> Self {
        Self {
            signing_key: hmac::Key::new(hmac::HMAC_SHA256, key_material),
            key_id: "picloud-hmac-1".to_string(),
            identities: Arc::new(RwLock::new(HashMap::new())),
            app_registrations: Arc::new(RwLock::new(HashMap::new())),
            passkeys: Arc::new(RwLock::new(HashMap::new())),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            pending_device_flows: Arc::new(RwLock::new(HashMap::new())),
            enrollment_tokens: Arc::new(RwLock::new(HashMap::new())),
            _iri_builder: IriBuilder::new(domain.clone()),
            cluster_domain: domain,
            token_ttl_secs,
            challenge_ttl_secs: 300,
            sparql_query: None,
        }
    }

    /// Inject a SPARQL query function for OWL role inheritance (ADR-051).
    /// Called from the composition root after the RDF store is initialized.
    pub fn set_sparql_query(&mut self, query_fn: SparqlQueryFn) {
        self.sparql_query = Some(query_fn);
    }

    /// Hash a client secret using SHA-256 for storage.
    fn hash_secret(secret: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(secret.as_bytes());
        let result = hasher.finalize();
        URL_SAFE_NO_PAD.encode(result)
    }

    /// Generate a random client secret.
    fn generate_secret() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        URL_SAFE_NO_PAD.encode(&bytes)
    }

    /// Generate random bytes and return them as base64url-encoded string.
    fn random_base64(len: usize) -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        URL_SAFE_NO_PAD.encode(&bytes)
    }

    /// Generate random bytes (raw).
    fn random_bytes(len: usize) -> Vec<u8> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        (0..len).map(|_| rng.gen()).collect()
    }

    /// Store an enrollment token for later use.
    pub async fn store_enrollment_token(&self, token: picloud_domain::identity::EnrollmentToken) {
        let key = token.token.clone();
        self.enrollment_tokens
            .write()
            .await
            .insert(key, StoredEnrollmentToken { token });
    }

    /// Register a passkey for an identity (used internally and in tests).
    pub async fn store_passkey(&self, identity_iri: &ResourceIri, passkey: RegisteredPasskey) {
        let key = identity_iri.as_str().to_string();
        let mut store = self.passkeys.write().await;
        store.entry(key).or_default().push(passkey);
    }

    /// Get all passkeys for an identity.
    pub async fn get_passkeys(&self, identity_iri: &ResourceIri) -> Vec<RegisteredPasskey> {
        let store = self.passkeys.read().await;
        store
            .get(identity_iri.as_str())
            .cloned()
            .unwrap_or_default()
    }

    /// Token exchange (RFC 8693 / ADR-051) — exchange a user token for a
    /// new token with a specific audience (on-behalf-of flow).
    pub async fn token_exchange(
        &self,
        subject_token: &str,
        audience: Option<&str>,
        scope: Option<&str>,
    ) -> Result<picloud_domain::identity::TokenResponse> {
        // Validate the incoming subject token
        let original_claims = self.verify_token(subject_token)?;

        let now = Utc::now();
        let scopes: Vec<String> = scope
            .map(|s| s.split_whitespace().map(|x| x.to_string()).collect())
            .unwrap_or_default();

        let exchanged = TokenClaims {
            identity_iri: original_claims.identity_iri.clone(),
            product: original_claims.product.clone(),
            roles: original_claims.roles.clone(),
            issued_at: now.timestamp(),
            expires_at: (now + Duration::seconds(self.token_ttl_secs)).timestamp(),
            aud: audience.map(|a| a.to_string()),
            scopes,
            permissions: original_claims.permissions.clone(),
            actor: Some(original_claims.identity_iri.clone()),
        };

        let token = self.sign_token(&exchanged)?;
        debug!(
            "Token exchange for {} -> audience {:?}",
            original_claims.identity_iri, audience
        );

        Ok(picloud_domain::identity::TokenResponse {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_in: self.token_ttl_secs,
            scope: scope.map(|s| s.to_string()),
        })
    }

    /// Issue a token with explicit audience and scopes (ADR-051).
    pub async fn issue_token_with_audience(
        &self,
        identity_iri: &ResourceIri,
        audience: &str,
        scopes: Vec<String>,
    ) -> Result<String> {
        let roles = {
            let store = self.identities.read().await;
            store
                .get(identity_iri.as_str())
                .map(|s| s.roles.clone())
                .unwrap_or_default()
        };

        // Extract product name from audience URL (e.g. ".../products/photo-app")
        let product = audience
            .split("/products/")
            .nth(1)
            .map(|s| s.split('/').next().unwrap_or(s).to_string());

        let now = Utc::now();
        let claims = TokenClaims {
            identity_iri: identity_iri.as_str().to_string(),
            product,
            roles,
            issued_at: now.timestamp(),
            expires_at: (now + Duration::seconds(self.token_ttl_secs)).timestamp(),
            aud: Some(audience.to_string()),
            scopes,
            permissions: Vec::new(),
            actor: None,
        };

        self.sign_token(&claims)
    }

    /// Register an identity so that tokens issued for it carry the correct roles.
    pub async fn register_identity(&self, iri: ResourceIri, roles: Vec<String>) {
        let key = iri.as_str().to_string();
        let stored = StoredIdentity {
            iri,
            roles,
        };
        debug!("Registered identity: {}", key);
        self.identities.write().await.insert(key, stored);
    }

    /// Sign a payload and return base64(json) + "." + base64(signature).
    fn sign_token(&self, claims: &TokenClaims) -> Result<String> {
        let json = serde_json::to_vec(claims)
            .map_err(|e| PiCloudError::Internal(format!("Failed to serialize claims: {e}")))?;
        let payload = URL_SAFE_NO_PAD.encode(&json);
        let tag = hmac::sign(&self.signing_key, payload.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(tag.as_ref());
        Ok(format!("{payload}.{signature}"))
    }

    /// Verify and decode a token, returning the claims.
    fn verify_token(&self, token: &str) -> Result<TokenClaims> {
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        if parts.len() != 2 {
            return Err(PiCloudError::Unauthenticated);
        }
        let (payload, signature) = (parts[0], parts[1]);

        // Verify HMAC
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| PiCloudError::Unauthenticated)?;
        hmac::verify(&self.signing_key, payload.as_bytes(), &sig_bytes)
            .map_err(|_| PiCloudError::Unauthenticated)?;

        // Decode claims
        let json_bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| PiCloudError::Unauthenticated)?;
        let claims: TokenClaims = serde_json::from_slice(&json_bytes)
            .map_err(|_| PiCloudError::Unauthenticated)?;

        // Check expiry
        let now = Utc::now().timestamp();
        if now >= claims.expires_at {
            warn!("Token expired for identity: {}", claims.identity_iri);
            return Err(PiCloudError::Unauthenticated);
        }

        Ok(claims)
    }
}

#[async_trait]
impl IdentityProvider for LocalIdentityProvider {
    async fn issue_token(
        &self,
        identity_iri: &ResourceIri,
        product: Option<&str>,
    ) -> Result<String> {
        let roles = {
            let store = self.identities.read().await;
            store
                .get(identity_iri.as_str())
                .map(|s| s.roles.clone())
                .unwrap_or_default()
        };

        let aud = product.map(|p| {
            format!("https://{}/products/{}", self.cluster_domain.0, p)
        });

        let now = Utc::now();
        let claims = TokenClaims {
            identity_iri: identity_iri.as_str().to_string(),
            product: product.map(|s| s.to_string()),
            roles,
            issued_at: now.timestamp(),
            expires_at: (now + Duration::seconds(self.token_ttl_secs)).timestamp(),
            aud,
            scopes: Vec::new(),
            permissions: Vec::new(),
            actor: None,
        };

        debug!("Issuing token for {}", identity_iri);
        self.sign_token(&claims)
    }

    async fn validate_token(&self, token: &str) -> Result<ValidatedIdentity> {
        let claims = self.verify_token(token)?;
        let identity_iri = ResourceIri::new(&claims.identity_iri)?;

        Ok(ValidatedIdentity {
            identity_iri,
            product: claims.product,
            roles: claims.roles,
            audience: claims.aud,
            scopes: claims.scopes,
            permissions: claims.permissions,
        })
    }

    async fn issue_workload_certificate(
        &self,
        workload_iri: &ResourceIri,
    ) -> Result<WorkloadCertificate> {
        debug!("Issuing workload certificate for {}", workload_iri);

        let mut params =
            rcgen::CertificateParams::new(vec![workload_iri.as_str().to_string()]).map_err(
                |e| PiCloudError::TlsCertificateError {
                    reason: format!("Failed to create cert params: {e}"),
                },
            )?;

        let expires_at = Utc::now() + Duration::days(90);
        params.not_before = rcgen::date_time_ymd(2024, 1, 1);
        let expire_year = expires_at.format("%Y").to_string().parse::<i32>().unwrap_or(2026);
        let expire_month = expires_at.format("%m").to_string().parse::<u8>().unwrap_or(1);
        let expire_day = expires_at.format("%d").to_string().parse::<u8>().unwrap_or(1);
        params.not_after = rcgen::date_time_ymd(expire_year, expire_month, expire_day);

        let key_pair =
            rcgen::KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("Failed to generate key pair: {e}"),
            })?;

        let cert =
            params
                .self_signed(&key_pair)
                .map_err(|e| PiCloudError::TlsCertificateError {
                    reason: format!("Failed to self-sign certificate: {e}"),
                })?;

        Ok(WorkloadCertificate {
            certificate_pem: cert.pem(),
            private_key_pem: key_pair.serialize_pem(),
            expires_at,
        })
    }

    async fn oidc_discovery(&self) -> Result<OidcDiscoveryDocument> {
        let issuer = format!("https://{}", self.cluster_domain.0);
        Ok(OidcDiscoveryDocument {
            issuer: issuer.clone(),
            authorization_endpoint: format!("{}/auth/authorize", issuer),
            token_endpoint: format!("{}/auth/token", issuer),
            jwks_uri: format!("{}/.well-known/jwks.json", issuer),
            response_types_supported: vec!["code".to_string()],
            subject_types_supported: vec!["public".to_string()],
            id_token_signing_alg_values_supported: vec!["HS256".to_string()],
            grant_types_supported: vec![
                "client_credentials".to_string(),
                "authorization_code".to_string(),
            ],
            token_endpoint_auth_methods_supported: vec![
                "client_secret_post".to_string(),
                "client_secret_basic".to_string(),
            ],
            scopes_supported: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
        })
    }

    async fn jwks(&self) -> Result<JsonWebKeySet> {
        // For HMAC-SHA256, we expose the key metadata but not the key itself.
        // Clients that need to verify tokens must use the token introspection
        // endpoint or validate via the platform API.
        Ok(JsonWebKeySet {
            keys: vec![JsonWebKey {
                kty: "oct".to_string(),
                kid: self.key_id.clone(),
                alg: "HS256".to_string(),
                key_use: "sig".to_string(),
                k: None, // HMAC secret is never exposed
            }],
        })
    }

    async fn client_credentials_token(
        &self,
        client_id: &str,
        client_secret: &str,
        scope: Option<&str>,
    ) -> Result<TokenResponse> {
        let store = self.app_registrations.read().await;
        let app = store.get(client_id).ok_or_else(|| {
            warn!("Client credentials auth failed: unknown client_id {}", client_id);
            PiCloudError::Unauthenticated
        })?;

        // Verify secret
        let secret_hash = Self::hash_secret(client_secret);
        if secret_hash != app.registration.client_secret_hash {
            warn!("Client credentials auth failed: bad secret for client_id {}", client_id);
            return Err(PiCloudError::Unauthenticated);
        }

        // Validate requested scopes against registered scopes (ADR-051)
        let granted_scopes: Vec<String> = if let Some(scope_str) = scope {
            let requested: Vec<&str> = scope_str.split_whitespace().collect();
            for req_scope in &requested {
                if !app.registration.scopes.contains(&req_scope.to_string()) {
                    warn!(
                        "Client {} requested unregistered scope: {}",
                        client_id, req_scope
                    );
                    return Err(PiCloudError::PermissionDenied(format!(
                        "scope '{}' is not registered for this client",
                        req_scope
                    )));
                }
            }
            requested.into_iter().map(|s| s.to_string()).collect()
        } else {
            // No scope requested — grant all registered scopes
            app.registration.scopes.clone()
        };

        // Issue a token with validated scopes
        let identity_iri = app.registration.product_iri.clone();
        let product_name = app.product_name.clone();
        drop(store); // Release read lock before async call

        let audience = format!(
            "https://{}/products/{}",
            self.cluster_domain.0, product_name
        );
        let token = self
            .issue_token_with_audience(&identity_iri, &audience, granted_scopes.clone())
            .await?;

        Ok(TokenResponse {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_in: self.token_ttl_secs,
            scope: Some(granted_scopes.join(" ")),
        })
    }

    async fn register_app(
        &self,
        product_iri: &ResourceIri,
        redirect_uris: Vec<String>,
        scopes: Vec<String>,
    ) -> Result<AppRegistration> {
        let client_id = format!("picloud-{}", uuid::Uuid::new_v4());
        let client_secret = Self::generate_secret();
        let client_secret_hash = Self::hash_secret(&client_secret);

        // Extract product name from IRI (last segment)
        let product_name = product_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string();

        let registration = AppRegistration {
            id: uuid::Uuid::new_v4(),
            client_id: client_id.clone(),
            client_secret_hash: client_secret_hash.clone(),
            product_iri: product_iri.clone(),
            redirect_uris,
            scopes,
        };

        let stored = StoredAppRegistration {
            registration: registration.clone(),
            product_name,
        };

        self.app_registrations
            .write()
            .await
            .insert(client_id, stored);

        debug!("Registered app for product: {}", product_iri);

        // Return the registration with the plaintext secret hash
        // The caller should return the plaintext secret to the user once.
        // We store only the hash. For the response, we put the plaintext secret
        // in client_secret_hash field — the caller (HTTP handler) will know
        // to present it as "client_secret" in the response.
        Ok(AppRegistration {
            id: registration.id,
            client_id: registration.client_id,
            client_secret_hash: client_secret, // plaintext secret for one-time display
            product_iri: registration.product_iri,
            redirect_uris: registration.redirect_uris,
            scopes: registration.scopes,
        })
    }

    // ---- WebAuthn / passkey ceremonies ----

    async fn begin_registration(
        &self,
        identity_iri: &ResourceIri,
    ) -> Result<(ChallengeId, RegistrationChallenge)> {
        let challenge_bytes = Self::random_bytes(32);
        let challenge_b64 = URL_SAFE_NO_PAD.encode(&challenge_bytes);
        let challenge_id = Self::random_base64(16);

        // Extract user name from IRI (last path segment)
        let user_name = identity_iri
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string();

        let pending = PendingChallenge {
            identity_iri: identity_iri.clone(),
            challenge_bytes: challenge_bytes.clone(),
            kind: ChallengeKind::Registration,
            expires_at: Utc::now() + Duration::seconds(self.challenge_ttl_secs),
        };

        self.pending_challenges
            .write()
            .await
            .insert(challenge_id.clone(), pending);

        debug!("Started registration challenge {} for {}", challenge_id, identity_iri);

        let options = RegistrationChallenge {
            challenge: challenge_b64,
            rp_id: self.cluster_domain.0.clone(),
            rp_name: "PiCloud".to_string(),
            user_id: URL_SAFE_NO_PAD.encode(identity_iri.as_str().as_bytes()),
            user_name,
            timeout_ms: (self.challenge_ttl_secs as u64) * 1000,
        };

        Ok((challenge_id, options))
    }

    async fn complete_registration(
        &self,
        challenge_id: &str,
        response: RegistrationResponse,
    ) -> Result<RegisteredPasskey> {
        // Remove and validate the pending challenge
        let pending = self
            .pending_challenges
            .write()
            .await
            .remove(challenge_id)
            .ok_or_else(|| PiCloudError::PasskeyChallengeFailed {
                reason: "unknown or expired challenge ID".to_string(),
            })?;

        if pending.kind != ChallengeKind::Registration {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "challenge is not a registration challenge".to_string(),
            });
        }

        if Utc::now() >= pending.expires_at {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "challenge has expired".to_string(),
            });
        }

        // Decode the public key from the response
        let public_key_bytes = URL_SAFE_NO_PAD
            .decode(&response.public_key)
            .map_err(|_| PiCloudError::PasskeyChallengeFailed {
                reason: "invalid public key encoding".to_string(),
            })?;

        let passkey = RegisteredPasskey {
            credential_id: response.credential_id.clone(),
            public_key: public_key_bytes,
            aaguid: response.aaguid,
            registered_at: Utc::now(),
            last_used_at: None,
            display_name: response.display_name,
        };

        // Store the passkey
        self.store_passkey(&pending.identity_iri, passkey.clone()).await;

        // Ensure the identity is registered (if not already)
        {
            let store = self.identities.read().await;
            if !store.contains_key(pending.identity_iri.as_str()) {
                drop(store);
                self.register_identity(pending.identity_iri.clone(), vec![]).await;
            }
        }

        debug!(
            "Registered passkey {} for {}",
            response.credential_id,
            pending.identity_iri
        );

        Ok(passkey)
    }

    async fn begin_authentication(
        &self,
        identity_iri: &ResourceIri,
    ) -> Result<(ChallengeId, AuthenticationChallenge)> {
        let passkeys = self.get_passkeys(identity_iri).await;
        if passkeys.is_empty() {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "no passkeys registered for this identity".to_string(),
            });
        }

        let challenge_bytes = Self::random_bytes(32);
        let challenge_b64 = URL_SAFE_NO_PAD.encode(&challenge_bytes);
        let challenge_id = Self::random_base64(16);

        let pending = PendingChallenge {
            identity_iri: identity_iri.clone(),
            challenge_bytes,
            kind: ChallengeKind::Authentication,
            expires_at: Utc::now() + Duration::seconds(self.challenge_ttl_secs),
        };

        self.pending_challenges
            .write()
            .await
            .insert(challenge_id.clone(), pending);

        let allow_credentials = passkeys.iter().map(|p| p.credential_id.clone()).collect();

        debug!("Started authentication challenge {} for {}", challenge_id, identity_iri);

        let options = AuthenticationChallenge {
            challenge: challenge_b64,
            rp_id: self.cluster_domain.0.clone(),
            allow_credentials,
            timeout_ms: (self.challenge_ttl_secs as u64) * 1000,
        };

        Ok((challenge_id, options))
    }

    async fn complete_authentication(
        &self,
        challenge_id: &str,
        response: AuthenticationResponse,
    ) -> Result<String> {
        // Remove and validate the pending challenge
        let pending = self
            .pending_challenges
            .write()
            .await
            .remove(challenge_id)
            .ok_or_else(|| PiCloudError::PasskeyChallengeFailed {
                reason: "unknown or expired challenge ID".to_string(),
            })?;

        if pending.kind != ChallengeKind::Authentication {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "challenge is not an authentication challenge".to_string(),
            });
        }

        if Utc::now() >= pending.expires_at {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "challenge has expired".to_string(),
            });
        }

        // Verify the credential ID matches a registered passkey
        let passkeys = self.get_passkeys(&pending.identity_iri).await;
        let matching_passkey = passkeys
            .iter()
            .find(|p| p.credential_id == response.credential_id)
            .ok_or_else(|| PiCloudError::PasskeyChallengeFailed {
                reason: "credential ID does not match any registered passkey".to_string(),
            })?;

        // Verify the signature based on the format.
        // "webauthn" = real ECDSA-P256 from a FIDO2 authenticator (YubiKey etc.)
        // "hmac"     = simplified HMAC-SHA256 for testing without hardware
        if response.signature_format == "webauthn" {
            // Real WebAuthn ECDSA verification.
            // The authenticator signs: authenticator_data || SHA256(client_data_json)
            // The client sends authenticator_data, client_data_json (or client_data_hash),
            // and the DER-encoded ECDSA signature — all base64url-encoded.
            let auth_data_bytes = response
                .authenticator_data
                .as_ref()
                .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
                .ok_or_else(|| PiCloudError::PasskeyChallengeFailed {
                    reason: "webauthn format requires authenticator_data".to_string(),
                })?;

            // Compute the client data hash. If client_data_json is provided, hash it;
            // otherwise the field itself is treated as the pre-computed hash.
            let client_data_hash = {
                use sha2::{Digest, Sha256};
                if let Some(ref cdj) = response.client_data_json {
                    let cdj_bytes = URL_SAFE_NO_PAD.decode(cdj).map_err(|_| {
                        PiCloudError::PasskeyChallengeFailed {
                            reason: "invalid client_data_json encoding".to_string(),
                        }
                    })?;
                    let mut hasher = Sha256::new();
                    hasher.update(&cdj_bytes);
                    hasher.finalize().to_vec()
                } else {
                    // Fallback: use the challenge bytes directly as the hash
                    // (for CLI direct mode where there is no real client_data_json)
                    let mut hasher = Sha256::new();
                    hasher.update(&pending.challenge_bytes);
                    hasher.finalize().to_vec()
                }
            };

            // The signed message is authenticator_data || client_data_hash
            let mut signed_message = auth_data_bytes;
            signed_message.extend_from_slice(&client_data_hash);

            let signature_bytes = URL_SAFE_NO_PAD.decode(&response.signature).map_err(|_| {
                PiCloudError::PasskeyChallengeFailed {
                    reason: "invalid signature encoding".to_string(),
                }
            })?;

            // Parse the stored COSE public key to extract the P-256 x,y coordinates.
            // COSE_Key for ES256: { 1: 2 (kty=EC2), 3: -7 (alg=ES256), -1: 1 (crv=P-256),
            //                       -2: x (32 bytes), -3: y (32 bytes) }
            // We support two storage formats:
            //   a) Raw 65-byte uncompressed point: 0x04 || x || y
            //   b) CBOR-encoded COSE_Key (we extract x,y from it)
            let pk_bytes = &matching_passkey.public_key;
            let (x_bytes, y_bytes) = if pk_bytes.len() == 65 && pk_bytes[0] == 0x04 {
                // Uncompressed EC point: 0x04 || x(32) || y(32)
                (pk_bytes[1..33].to_vec(), pk_bytes[33..65].to_vec())
            } else if pk_bytes.len() == 64 {
                // Raw x||y (64 bytes)
                (pk_bytes[0..32].to_vec(), pk_bytes[32..64].to_vec())
            } else {
                // Attempt CBOR-encoded COSE_Key decode
                // COSE_Key for ES256: { 1: 2 (kty=EC2), 3: -7 (alg=ES256),
                //   -1: 1 (crv=P-256), -2: x (32 bytes), -3: y (32 bytes) }
                match ciborium::from_reader::<ciborium::Value, _>(
                    std::io::Cursor::new(pk_bytes),
                ) {
                    Ok(ciborium::Value::Map(entries)) => {
                        let get_bytes = |key: ciborium::Value| -> Option<Vec<u8>> {
                            entries.iter().find_map(|(k, v)| {
                                if *k == key {
                                    if let ciborium::Value::Bytes(b) = v {
                                        Some(b.clone())
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                        };

                        // Validate kty=2 (EC2)
                        let kty = entries.iter().find_map(|(k, v)| {
                            if *k == ciborium::Value::Integer(1.into()) {
                                if let ciborium::Value::Integer(i) = v {
                                    Some(i128::from(*i))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        });
                        if kty != Some(2) {
                            return Err(PiCloudError::PasskeyChallengeFailed {
                                reason: format!("COSE_Key kty is not EC2 (got {:?})", kty),
                            });
                        }

                        // Extract x (-2) and y (-3) coordinates
                        let x = get_bytes(ciborium::Value::Integer((-2i128).try_into().unwrap()))
                            .ok_or_else(|| PiCloudError::PasskeyChallengeFailed {
                                reason: "COSE_Key missing x coordinate (-2)".to_string(),
                            })?;
                        let y = get_bytes(ciborium::Value::Integer((-3i128).try_into().unwrap()))
                            .ok_or_else(|| PiCloudError::PasskeyChallengeFailed {
                                reason: "COSE_Key missing y coordinate (-3)".to_string(),
                            })?;

                        if x.len() != 32 || y.len() != 32 {
                            return Err(PiCloudError::PasskeyChallengeFailed {
                                reason: format!(
                                    "COSE_Key coordinate size mismatch (x={}, y={})",
                                    x.len(),
                                    y.len()
                                ),
                            });
                        }

                        (x, y)
                    }
                    _ => {
                        return Err(PiCloudError::PasskeyChallengeFailed {
                            reason: format!(
                                "unsupported public key format (length={}, not CBOR map)",
                                pk_bytes.len()
                            ),
                        });
                    }
                }
            };

            // Build the P-256 verifying key from affine coordinates
            use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
            use p256::EncodedPoint;

            let encoded_point = EncodedPoint::from_affine_coordinates(
                p256::FieldBytes::from_slice(&x_bytes),
                p256::FieldBytes::from_slice(&y_bytes),
                false, // uncompressed
            );
            let verifying_key = VerifyingKey::from_encoded_point(&encoded_point).map_err(|e| {
                PiCloudError::PasskeyChallengeFailed {
                    reason: format!("invalid public key: {e}"),
                }
            })?;

            // Parse the DER-encoded signature
            let sig = Signature::from_der(&signature_bytes).map_err(|e| {
                PiCloudError::PasskeyChallengeFailed {
                    reason: format!("invalid DER signature: {e}"),
                }
            })?;

            verifying_key.verify(&signed_message, &sig).map_err(|_| {
                warn!(
                    "WebAuthn ECDSA verification failed for {}",
                    pending.identity_iri
                );
                PiCloudError::PasskeyChallengeFailed {
                    reason: "ECDSA signature verification failed".to_string(),
                }
            })?;
        } else {
            // Legacy HMAC-based simplified verification.
            // The client signs: HMAC-SHA256(challenge_bytes, credential_public_key)
            let expected_tag = hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA256, &matching_passkey.public_key),
                &pending.challenge_bytes,
            );
            let expected_sig = URL_SAFE_NO_PAD.encode(expected_tag.as_ref());

            if response.signature != expected_sig {
                warn!(
                    "Authentication failed for {} — HMAC signature mismatch",
                    pending.identity_iri
                );
                return Err(PiCloudError::PasskeyChallengeFailed {
                    reason: "signature verification failed".to_string(),
                });
            }
        }

        // Update last_used_at
        {
            let mut store = self.passkeys.write().await;
            if let Some(passkeys) = store.get_mut(pending.identity_iri.as_str()) {
                if let Some(pk) = passkeys.iter_mut().find(|p| p.credential_id == response.credential_id) {
                    pk.last_used_at = Some(Utc::now());
                }
            }
        }

        debug!("Authentication succeeded for {}", pending.identity_iri);

        // Issue a token for the authenticated identity
        self.issue_token(&pending.identity_iri, None).await
    }

    async fn enroll_with_token(
        &self,
        token: &str,
    ) -> Result<(ChallengeId, RegistrationChallenge)> {
        // Find and validate the enrollment token
        let mut tokens = self.enrollment_tokens.write().await;
        let stored = tokens.get_mut(token).ok_or_else(|| {
            PiCloudError::PasskeyChallengeFailed {
                reason: "invalid enrollment token".to_string(),
            }
        })?;

        if !stored.token.is_valid() {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "enrollment token is expired or already used".to_string(),
            });
        }

        // Mark the token as used
        stored.token.used = true;

        // Determine the identity IRI for enrollment
        let identity_iri = match &stored.token.target_identity {
            Some(iri) => iri.clone(),
            None => {
                // For bootstrap tokens, create a new admin identity
                let iri_builder = IriBuilder::new(self.cluster_domain.clone());
                let admin_id = uuid::Uuid::new_v4();
                let iri = iri_builder.resource("platform", "identities", &admin_id.to_string());
                // Register as admin
                drop(tokens);
                self.register_identity(iri.clone(), vec!["admin".to_string()]).await;
                return self.begin_registration(&iri).await;
            }
        };

        drop(tokens);
        self.begin_registration(&identity_iri).await
    }

    async fn begin_device_flow(&self) -> Result<DeviceFlowResponse> {
        let device_code = Self::random_base64(32);
        let expires_in_secs: u64 = 600; // 10 minutes

        let pending = PendingDeviceFlow {
            expires_at: Utc::now() + Duration::seconds(expires_in_secs as i64),
            completed_token: None,
            identity_iri: None,
        };

        self.pending_device_flows
            .write()
            .await
            .insert(device_code.clone(), pending);

        let verification_url = format!(
            "https://{}/auth/device?code={}",
            self.cluster_domain.0, device_code
        );

        debug!("Started device flow: {}", device_code);

        Ok(DeviceFlowResponse {
            device_code,
            verification_url,
            interval_secs: 5,
            expires_in_secs,
        })
    }

    async fn poll_device_flow(
        &self,
        device_code: &str,
    ) -> Result<DeviceFlowPollResult> {
        let store = self.pending_device_flows.read().await;
        let pending = store.get(device_code).ok_or_else(|| {
            PiCloudError::PasskeyChallengeFailed {
                reason: "unknown device code".to_string(),
            }
        })?;

        if Utc::now() >= pending.expires_at {
            return Ok(DeviceFlowPollResult::Expired);
        }

        match &pending.completed_token {
            Some(token) => Ok(DeviceFlowPollResult::Complete {
                access_token: token.clone(),
                token_type: "Bearer".to_string(),
                expires_in: self.token_ttl_secs,
            }),
            None => Ok(DeviceFlowPollResult::Pending),
        }
    }

    async fn complete_device_flow(
        &self,
        device_code: &str,
        identity_iri: &ResourceIri,
    ) -> Result<()> {
        let token = self.issue_token(identity_iri, None).await?;

        let mut store = self.pending_device_flows.write().await;
        let pending = store.get_mut(device_code).ok_or_else(|| {
            PiCloudError::PasskeyChallengeFailed {
                reason: "unknown device code".to_string(),
            }
        })?;

        if Utc::now() >= pending.expires_at {
            return Err(PiCloudError::PasskeyChallengeFailed {
                reason: "device code has expired".to_string(),
            });
        }

        pending.completed_token = Some(token);
        pending.identity_iri = Some(identity_iri.clone());

        debug!("Completed device flow for {}", identity_iri);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TokenExchange trait implementation (ADR-051)
// ---------------------------------------------------------------------------

#[async_trait]
impl picloud_domain::traits::TokenExchange for LocalIdentityProvider {
    async fn token_exchange(
        &self,
        request: &picloud_domain::identity::TokenExchangeRequest,
    ) -> Result<picloud_domain::identity::TokenResponse> {
        self.token_exchange(
            &request.subject_token,
            request.audience.as_deref(),
            request.scope.as_deref(),
        )
        .await
    }

    async fn resolve_roles(
        &self,
        identity_iri: &ResourceIri,
        product: &str,
    ) -> Result<picloud_domain::identity::ResolvedTokenClaims> {
        // Get the identity's direct roles
        let direct_roles = {
            let store = self.identities.read().await;
            store
                .get(identity_iri.as_str())
                .map(|s| s.roles.clone())
                .unwrap_or_default()
        };

        // Expand roles via rdfs:subClassOf* traversal using the RDF graph
        let mut all_roles: std::collections::HashSet<String> =
            direct_roles.iter().cloned().collect();

        if let Some(ref sparql_fn) = self.sparql_query {
            for role in &direct_roles {
                // Query for all superclasses via transitive rdfs:subClassOf
                let query = format!(
                    r#"
                    PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
                    SELECT ?super WHERE {{
                        <{role}> rdfs:subClassOf* ?super .
                        FILTER(?super != <{role}>)
                    }}
                    "#
                );
                match sparql_fn(query).await {
                    Ok(result) => {
                        for binding in &result.bindings {
                            if let Some(super_role) = binding.get("super")
                                .and_then(|v| v.as_str())
                            {
                                all_roles.insert(super_role.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        debug!("OWL role inheritance query failed for {}: {}", role, e);
                        // Fall through — use direct roles only
                    }
                }
            }
        }

        // Flatten permissions from all resolved roles
        let mut permissions = Vec::new();
        if let Some(ref sparql_fn) = self.sparql_query {
            let roles_filter = all_roles
                .iter()
                .map(|r| format!("<{r}>"))
                .collect::<Vec<_>>()
                .join(" ");

            if !roles_filter.is_empty() {
                let perm_query = format!(
                    r#"
                    PREFIX pc: <https://picloud.local/ontology#>
                    SELECT ?permission WHERE {{
                        VALUES ?role {{ {roles_filter} }}
                        ?role pc:hasPermission ?permission .
                    }}
                    "#
                );
                if let Ok(result) = sparql_fn(perm_query).await {
                    for binding in &result.bindings {
                        if let Some(perm) = binding.get("permission")
                            .and_then(|v| v.as_str())
                        {
                            permissions.push(perm.to_string());
                        }
                    }
                }
            }
        }

        Ok(picloud_domain::identity::ResolvedTokenClaims {
            subject: identity_iri.as_str().to_string(),
            audience: format!(
                "https://{}/products/{product}",
                self.cluster_domain.0
            ),
            issuer: format!("https://{}", self.cluster_domain.0),
            scopes: Vec::new(),
            roles: all_roles.into_iter().collect(),
            permissions,
            custom: std::collections::HashMap::new(),
            actor: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider() -> LocalIdentityProvider {
        LocalIdentityProvider::new(b"test-secret-key-for-hmac-signing", ClusterDomain::default())
    }

    fn test_provider_short_ttl() -> LocalIdentityProvider {
        LocalIdentityProvider::with_ttl(
            b"test-secret-key-for-hmac-signing",
            ClusterDomain::default(),
            -1, // already expired
        )
    }

    fn test_iri() -> ResourceIri {
        ResourceIri::new("https://picloud.local/identities/test-user").unwrap()
    }

    #[tokio::test]
    async fn issue_and_validate_token_round_trip() {
        let provider = test_provider();
        let iri = test_iri();

        provider
            .register_identity(iri.clone(), vec!["admin".to_string(), "reader".to_string()])
            .await;

        let token = provider
            .issue_token(&iri, Some("photo-app"))
            .await
            .expect("issue_token should succeed");

        let validated = provider
            .validate_token(&token)
            .await
            .expect("validate_token should succeed");

        assert_eq!(validated.identity_iri, iri);
        assert_eq!(validated.product, Some("photo-app".to_string()));
        assert_eq!(validated.roles, vec!["admin", "reader"]);
    }

    #[tokio::test]
    async fn expired_token_is_rejected() {
        let provider = test_provider_short_ttl();
        let iri = test_iri();

        let token = provider
            .issue_token(&iri, None)
            .await
            .expect("issue_token should succeed");

        let result = provider.validate_token(&token).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PiCloudError::Unauthenticated));
    }

    #[tokio::test]
    async fn tampered_token_is_rejected() {
        let provider = test_provider();
        let iri = test_iri();

        let token = provider
            .issue_token(&iri, None)
            .await
            .expect("issue_token should succeed");

        // Tamper with the payload portion
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        let tampered = format!("{}x.{}", parts[0], parts[1]);

        let result = provider.validate_token(&tampered).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PiCloudError::Unauthenticated));
    }

    #[tokio::test]
    async fn workload_certificate_generation() {
        let provider = test_provider();
        let workload_iri =
            ResourceIri::new("https://picloud.local/products/photo-app/containers/api-server")
                .unwrap();

        let cert = provider
            .issue_workload_certificate(&workload_iri)
            .await
            .expect("issue_workload_certificate should succeed");

        assert!(cert.certificate_pem.contains("BEGIN CERTIFICATE"));
        assert!(cert.private_key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(cert.expires_at > Utc::now());
    }

    #[tokio::test]
    async fn token_without_registered_identity_has_no_roles() {
        let provider = test_provider();
        let iri = test_iri();

        let token = provider
            .issue_token(&iri, None)
            .await
            .expect("issue_token should succeed");

        let validated = provider
            .validate_token(&token)
            .await
            .expect("validate_token should succeed");

        assert!(validated.roles.is_empty());
    }

    // -- OIDC tests --

    #[tokio::test]
    async fn oidc_discovery_returns_valid_document() {
        let provider = test_provider();
        let doc = provider
            .oidc_discovery()
            .await
            .expect("oidc_discovery should succeed");

        assert_eq!(doc.issuer, "https://picloud.local");
        assert_eq!(
            doc.authorization_endpoint,
            "https://picloud.local/auth/authorize"
        );
        assert_eq!(doc.token_endpoint, "https://picloud.local/auth/token");
        assert_eq!(
            doc.jwks_uri,
            "https://picloud.local/.well-known/jwks.json"
        );
        assert!(doc.grant_types_supported.contains(&"client_credentials".to_string()));
        assert!(doc.id_token_signing_alg_values_supported.contains(&"HS256".to_string()));
    }

    #[tokio::test]
    async fn jwks_returns_key_set() {
        let provider = test_provider();
        let jwks = provider.jwks().await.expect("jwks should succeed");

        assert_eq!(jwks.keys.len(), 1);
        let key = &jwks.keys[0];
        assert_eq!(key.kty, "oct");
        assert_eq!(key.alg, "HS256");
        assert_eq!(key.key_use, "sig");
        assert_eq!(key.kid, "picloud-hmac-1");
        // HMAC secret must never be exposed
        assert!(key.k.is_none());
    }

    #[tokio::test]
    async fn register_app_and_client_credentials_flow() {
        let provider = test_provider();
        let product_iri =
            ResourceIri::new("https://picloud.local/products/photo-app").unwrap();

        // Register an app
        let app = provider
            .register_app(&product_iri, vec![], vec!["openid".to_string()])
            .await
            .expect("register_app should succeed");

        assert!(app.client_id.starts_with("picloud-"));
        assert!(!app.client_secret_hash.is_empty()); // contains plaintext secret at creation

        let client_secret = app.client_secret_hash.clone(); // plaintext secret from registration

        // Authenticate with client credentials
        let token_resp = provider
            .client_credentials_token(&app.client_id, &client_secret, Some("openid"))
            .await
            .expect("client_credentials_token should succeed");

        assert_eq!(token_resp.token_type, "Bearer");
        assert!(!token_resp.access_token.is_empty());
        assert_eq!(token_resp.expires_in, 3600);

        // Validate the issued token
        let validated = provider
            .validate_token(&token_resp.access_token)
            .await
            .expect("validate_token should succeed");

        assert_eq!(validated.identity_iri, product_iri);
        assert_eq!(validated.product, Some("photo-app".to_string()));
    }

    #[tokio::test]
    async fn client_credentials_with_wrong_secret_fails() {
        let provider = test_provider();
        let product_iri =
            ResourceIri::new("https://picloud.local/products/photo-app").unwrap();

        let app = provider
            .register_app(&product_iri, vec![], vec![])
            .await
            .expect("register_app should succeed");

        let result = provider
            .client_credentials_token(&app.client_id, "wrong-secret", None)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::Unauthenticated
        ));
    }

    #[tokio::test]
    async fn client_credentials_with_unknown_client_fails() {
        let provider = test_provider();

        let result = provider
            .client_credentials_token("unknown-client", "some-secret", None)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::Unauthenticated
        ));
    }

    // -- WebAuthn registration tests --

    #[tokio::test]
    async fn begin_registration_returns_challenge() {
        let provider = test_provider();
        let iri = test_iri();

        let (challenge_id, options) = provider
            .begin_registration(&iri)
            .await
            .expect("begin_registration should succeed");

        assert!(!challenge_id.is_empty());
        assert!(!options.challenge.is_empty());
        assert_eq!(options.rp_id, "picloud.local");
        assert_eq!(options.rp_name, "PiCloud");
        assert_eq!(options.user_name, "test-user");
        assert!(options.timeout_ms > 0);
    }

    #[tokio::test]
    async fn complete_registration_stores_passkey() {
        let provider = test_provider();
        let iri = test_iri();

        let (challenge_id, _options) = provider
            .begin_registration(&iri)
            .await
            .expect("begin_registration should succeed");

        let public_key = LocalIdentityProvider::random_bytes(32);
        let response = picloud_domain::identity::RegistrationResponse {
            credential_id: "test-cred-001".to_string(),
            public_key: URL_SAFE_NO_PAD.encode(&public_key),
            attestation: None,
            aaguid: Some("test-aaguid".to_string()),
            display_name: Some("My YubiKey".to_string()),
        };

        let passkey = provider
            .complete_registration(&challenge_id, response)
            .await
            .expect("complete_registration should succeed");

        assert_eq!(passkey.credential_id, "test-cred-001");
        assert_eq!(passkey.public_key, public_key);
        assert_eq!(passkey.aaguid, Some("test-aaguid".to_string()));
        assert_eq!(passkey.display_name, Some("My YubiKey".to_string()));

        // Verify the passkey was stored
        let stored = provider.get_passkeys(&iri).await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].credential_id, "test-cred-001");
    }

    #[tokio::test]
    async fn complete_registration_with_invalid_challenge_fails() {
        let provider = test_provider();

        let response = picloud_domain::identity::RegistrationResponse {
            credential_id: "test-cred".to_string(),
            public_key: URL_SAFE_NO_PAD.encode(&[1, 2, 3]),
            attestation: None,
            aaguid: None,
            display_name: None,
        };

        let result = provider
            .complete_registration("nonexistent-challenge", response)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::PasskeyChallengeFailed { .. }
        ));
    }

    #[tokio::test]
    async fn complete_registration_challenge_cannot_be_reused() {
        let provider = test_provider();
        let iri = test_iri();

        let (challenge_id, _options) = provider
            .begin_registration(&iri)
            .await
            .unwrap();

        let public_key = LocalIdentityProvider::random_bytes(32);
        let response = picloud_domain::identity::RegistrationResponse {
            credential_id: "cred-1".to_string(),
            public_key: URL_SAFE_NO_PAD.encode(&public_key),
            attestation: None,
            aaguid: None,
            display_name: None,
        };

        // First completion succeeds
        provider
            .complete_registration(&challenge_id, response.clone())
            .await
            .expect("first complete should succeed");

        // Second completion with same challenge_id fails (it was consumed)
        let result = provider
            .complete_registration(&challenge_id, response)
            .await;
        assert!(result.is_err());
    }

    // -- WebAuthn authentication tests --

    async fn register_test_passkey(provider: &LocalIdentityProvider) -> (ResourceIri, Vec<u8>) {
        let iri = test_iri();
        let (challenge_id, _) = provider.begin_registration(&iri).await.unwrap();

        let public_key = LocalIdentityProvider::random_bytes(32);
        let response = picloud_domain::identity::RegistrationResponse {
            credential_id: "auth-test-cred".to_string(),
            public_key: URL_SAFE_NO_PAD.encode(&public_key),
            attestation: None,
            aaguid: None,
            display_name: None,
        };
        provider.complete_registration(&challenge_id, response).await.unwrap();
        (iri, public_key)
    }

    #[tokio::test]
    async fn begin_authentication_with_no_passkeys_fails() {
        let provider = test_provider();
        let iri = test_iri();

        let result = provider.begin_authentication(&iri).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::PasskeyChallengeFailed { .. }
        ));
    }

    #[tokio::test]
    async fn authentication_round_trip() {
        let provider = test_provider();
        let (iri, public_key) = register_test_passkey(&provider).await;

        // Begin authentication
        let (challenge_id, options) = provider
            .begin_authentication(&iri)
            .await
            .expect("begin_authentication should succeed");

        assert!(!options.challenge.is_empty());
        assert_eq!(options.allow_credentials, vec!["auth-test-cred"]);

        // Compute the expected signature: HMAC-SHA256(challenge_bytes, public_key)
        let challenge_bytes = URL_SAFE_NO_PAD.decode(&options.challenge).unwrap();
        let sig_key = hmac::Key::new(hmac::HMAC_SHA256, &public_key);
        let sig = hmac::sign(&sig_key, &challenge_bytes);
        let signature = URL_SAFE_NO_PAD.encode(sig.as_ref());

        let response = picloud_domain::identity::AuthenticationResponse {
            credential_id: "auth-test-cred".to_string(),
            signature,
            authenticator_data: None,
            client_data_json: None,
            signature_format: "hmac".to_string(),
        };

        let token = provider
            .complete_authentication(&challenge_id, response)
            .await
            .expect("complete_authentication should succeed");

        // Validate the returned token
        let validated = provider.validate_token(&token).await.unwrap();
        assert_eq!(validated.identity_iri, iri);
    }

    #[tokio::test]
    async fn authentication_with_wrong_signature_fails() {
        let provider = test_provider();
        let (iri, _public_key) = register_test_passkey(&provider).await;

        let (challenge_id, _options) = provider.begin_authentication(&iri).await.unwrap();

        let response = picloud_domain::identity::AuthenticationResponse {
            credential_id: "auth-test-cred".to_string(),
            signature: "wrong-signature-data".to_string(),
            authenticator_data: None,
            client_data_json: None,
            signature_format: "hmac".to_string(),
        };

        let result = provider.complete_authentication(&challenge_id, response).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::PasskeyChallengeFailed { .. }
        ));
    }

    #[tokio::test]
    async fn authentication_with_unknown_credential_fails() {
        let provider = test_provider();
        let (iri, _public_key) = register_test_passkey(&provider).await;

        let (challenge_id, _options) = provider.begin_authentication(&iri).await.unwrap();

        let response = picloud_domain::identity::AuthenticationResponse {
            credential_id: "unknown-cred".to_string(),
            signature: "doesnt-matter".to_string(),
            authenticator_data: None,
            client_data_json: None,
            signature_format: "hmac".to_string(),
        };

        let result = provider.complete_authentication(&challenge_id, response).await;
        assert!(result.is_err());
    }

    // -- Enrollment token tests --

    #[tokio::test]
    async fn enroll_with_valid_bootstrap_token() {
        let provider = test_provider();

        let token = picloud_domain::identity::EnrollmentToken {
            token: "bootstrap-token-123".to_string(),
            purpose: picloud_domain::identity::EnrollmentPurpose::Bootstrap,
            expires_at: Utc::now() + Duration::hours(1),
            used: false,
            target_identity: None,
        };
        provider.store_enrollment_token(token).await;

        let (challenge_id, options) = provider
            .enroll_with_token("bootstrap-token-123")
            .await
            .expect("enroll_with_token should succeed");

        assert!(!challenge_id.is_empty());
        assert!(!options.challenge.is_empty());
        assert_eq!(options.rp_id, "picloud.local");
    }

    #[tokio::test]
    async fn enroll_with_used_token_fails() {
        let provider = test_provider();

        let token = picloud_domain::identity::EnrollmentToken {
            token: "used-token".to_string(),
            purpose: picloud_domain::identity::EnrollmentPurpose::Bootstrap,
            expires_at: Utc::now() + Duration::hours(1),
            used: true,
            target_identity: None,
        };
        provider.store_enrollment_token(token).await;

        let result = provider.enroll_with_token("used-token").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn enroll_with_expired_token_fails() {
        let provider = test_provider();

        let token = picloud_domain::identity::EnrollmentToken {
            token: "expired-token".to_string(),
            purpose: picloud_domain::identity::EnrollmentPurpose::Bootstrap,
            expires_at: Utc::now() - Duration::hours(1),
            used: false,
            target_identity: None,
        };
        provider.store_enrollment_token(token).await;

        let result = provider.enroll_with_token("expired-token").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn enroll_with_passkey_reset_token() {
        let provider = test_provider();
        let iri = test_iri();

        let token = picloud_domain::identity::EnrollmentToken {
            token: "reset-token-456".to_string(),
            purpose: picloud_domain::identity::EnrollmentPurpose::PasskeyReset,
            expires_at: Utc::now() + Duration::hours(1),
            used: false,
            target_identity: Some(iri.clone()),
        };
        provider.store_enrollment_token(token).await;

        let (challenge_id, options) = provider
            .enroll_with_token("reset-token-456")
            .await
            .expect("enroll should succeed");

        assert!(!challenge_id.is_empty());
        // The user_name should come from the target identity
        assert_eq!(options.user_name, "test-user");
    }

    // -- Device flow tests --

    #[tokio::test]
    async fn device_flow_round_trip() {
        let provider = test_provider();
        let iri = test_iri();
        provider.register_identity(iri.clone(), vec!["user".to_string()]).await;

        // Begin device flow
        let flow = provider
            .begin_device_flow()
            .await
            .expect("begin_device_flow should succeed");

        assert!(!flow.device_code.is_empty());
        assert!(flow.verification_url.contains(&flow.device_code));
        assert_eq!(flow.interval_secs, 5);

        // Poll — should be pending
        let poll = provider.poll_device_flow(&flow.device_code).await.unwrap();
        assert!(matches!(poll, DeviceFlowPollResult::Pending));

        // Complete the device flow (simulates browser auth)
        provider
            .complete_device_flow(&flow.device_code, &iri)
            .await
            .expect("complete_device_flow should succeed");

        // Poll again — should be complete with a token
        let poll = provider.poll_device_flow(&flow.device_code).await.unwrap();
        match poll {
            DeviceFlowPollResult::Complete { access_token, token_type, expires_in } => {
                assert_eq!(token_type, "Bearer");
                assert!(expires_in > 0);
                // Validate the token
                let validated = provider.validate_token(&access_token).await.unwrap();
                assert_eq!(validated.identity_iri, iri);
                assert_eq!(validated.roles, vec!["user"]);
            }
            other => panic!("Expected Complete, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn device_flow_unknown_code_fails() {
        let provider = test_provider();

        let result = provider.poll_device_flow("nonexistent-code").await;
        assert!(result.is_err());
    }

    // -- ECDSA (WebAuthn) authentication tests --

    /// Register a passkey with a real P-256 public key, then authenticate using
    /// ECDSA signatures (signature_format = "webauthn").
    #[tokio::test]
    async fn webauthn_ecdsa_authentication_round_trip() {
        use p256::ecdsa::{signature::Signer, SigningKey};

        let provider = test_provider();
        let iri = test_iri();

        // Generate a P-256 key pair
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();

        // Get the uncompressed point (0x04 || x || y) — 65 bytes
        let encoded_point = verifying_key.to_encoded_point(false);
        let public_key_bytes = encoded_point.as_bytes().to_vec();
        assert_eq!(public_key_bytes.len(), 65);
        assert_eq!(public_key_bytes[0], 0x04);

        // Register the passkey with the ECDSA public key
        let (challenge_id, _options) = provider.begin_registration(&iri).await.unwrap();

        let reg_response = picloud_domain::identity::RegistrationResponse {
            credential_id: "ecdsa-cred-001".to_string(),
            public_key: URL_SAFE_NO_PAD.encode(&public_key_bytes),
            attestation: None,
            aaguid: None,
            display_name: Some("Test ECDSA Key".to_string()),
        };
        provider
            .complete_registration(&challenge_id, reg_response)
            .await
            .expect("registration should succeed");

        // Begin authentication
        let (challenge_id, options) = provider.begin_authentication(&iri).await.unwrap();
        assert_eq!(options.allow_credentials, vec!["ecdsa-cred-001"]);

        let challenge_bytes = URL_SAFE_NO_PAD.decode(&options.challenge).unwrap();

        // Build the authenticator data — a minimal 37-byte structure:
        //   rpIdHash (32 bytes) + flags (1 byte) + signCount (4 bytes)
        let rpid_hash = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(options.rp_id.as_bytes());
            h.finalize().to_vec()
        };
        let mut authenticator_data = rpid_hash;
        authenticator_data.push(0x01); // flags: UP=1
        authenticator_data.extend_from_slice(&[0, 0, 0, 1]); // signCount = 1

        // Build client data JSON
        let client_data_json = serde_json::to_vec(&serde_json::json!({
            "type": "webauthn.get",
            "challenge": options.challenge,
            "origin": format!("https://{}", options.rp_id),
            "crossOrigin": false,
        }))
        .unwrap();

        // Compute signed message: authenticator_data || SHA256(client_data_json)
        let client_data_hash = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(&client_data_json);
            h.finalize().to_vec()
        };
        let mut signed_message = authenticator_data.clone();
        signed_message.extend_from_slice(&client_data_hash);

        // Sign with P-256
        let signature: p256::ecdsa::DerSignature = signing_key.sign(&signed_message);

        let auth_response = picloud_domain::identity::AuthenticationResponse {
            credential_id: "ecdsa-cred-001".to_string(),
            signature: URL_SAFE_NO_PAD.encode(signature.as_bytes()),
            authenticator_data: Some(URL_SAFE_NO_PAD.encode(&authenticator_data)),
            client_data_json: Some(URL_SAFE_NO_PAD.encode(&client_data_json)),
            signature_format: "webauthn".to_string(),
        };

        let token = provider
            .complete_authentication(&challenge_id, auth_response)
            .await
            .expect("ECDSA authentication should succeed");

        // Validate the returned token
        let validated = provider.validate_token(&token).await.unwrap();
        assert_eq!(validated.identity_iri, iri);
    }

    #[tokio::test]
    async fn webauthn_ecdsa_wrong_signature_fails() {
        use p256::ecdsa::SigningKey;

        let provider = test_provider();
        let iri = test_iri();

        // Generate a P-256 key pair for registration
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        let encoded_point = verifying_key.to_encoded_point(false);
        let public_key_bytes = encoded_point.as_bytes().to_vec();

        // Register
        let (challenge_id, _) = provider.begin_registration(&iri).await.unwrap();
        let reg_response = picloud_domain::identity::RegistrationResponse {
            credential_id: "ecdsa-cred-002".to_string(),
            public_key: URL_SAFE_NO_PAD.encode(&public_key_bytes),
            attestation: None,
            aaguid: None,
            display_name: None,
        };
        provider
            .complete_registration(&challenge_id, reg_response)
            .await
            .unwrap();

        // Begin authentication
        let (challenge_id, options) = provider.begin_authentication(&iri).await.unwrap();

        let rpid_hash = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(options.rp_id.as_bytes());
            h.finalize().to_vec()
        };
        let mut authenticator_data = rpid_hash;
        authenticator_data.push(0x01);
        authenticator_data.extend_from_slice(&[0, 0, 0, 1]);

        // Use a DIFFERENT key to sign — verification must fail
        let wrong_key = SigningKey::random(&mut rand::thread_rng());
        use p256::ecdsa::signature::Signer;
        let sig: p256::ecdsa::DerSignature = wrong_key.sign(&authenticator_data);

        let auth_response = picloud_domain::identity::AuthenticationResponse {
            credential_id: "ecdsa-cred-002".to_string(),
            signature: URL_SAFE_NO_PAD.encode(sig.as_bytes()),
            authenticator_data: Some(URL_SAFE_NO_PAD.encode(&authenticator_data)),
            client_data_json: None,
            signature_format: "webauthn".to_string(),
        };

        let result = provider
            .complete_authentication(&challenge_id, auth_response)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::PasskeyChallengeFailed { .. }
        ));
    }
}
