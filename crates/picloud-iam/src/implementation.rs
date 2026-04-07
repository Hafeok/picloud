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
    AppRegistration, JsonWebKey, JsonWebKeySet, OidcDiscoveryDocument, TokenResponse,
};
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::traits::{IdentityProvider, ValidatedIdentity, WorkloadCertificate};

/// Internal token claims — serialized to JSON then base64-encoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenClaims {
    identity_iri: String,
    product: Option<String>,
    roles: Vec<String>,
    issued_at: i64,
    expires_at: i64,
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

/// Local identity provider backed by HMAC-SHA256 token signing and rcgen certificates.
pub struct LocalIdentityProvider {
    signing_key: hmac::Key,
    key_id: String,
    identities: Arc<RwLock<HashMap<String, StoredIdentity>>>,
    app_registrations: Arc<RwLock<HashMap<String, StoredAppRegistration>>>,
    _iri_builder: IriBuilder,
    cluster_domain: ClusterDomain,
    /// Token validity duration in seconds.
    token_ttl_secs: i64,
}

impl LocalIdentityProvider {
    /// Create a new provider with the given HMAC key material.
    pub fn new(key_material: &[u8], domain: ClusterDomain) -> Self {
        Self {
            signing_key: hmac::Key::new(hmac::HMAC_SHA256, key_material),
            key_id: "picloud-hmac-1".to_string(),
            identities: Arc::new(RwLock::new(HashMap::new())),
            app_registrations: Arc::new(RwLock::new(HashMap::new())),
            _iri_builder: IriBuilder::new(domain.clone()),
            cluster_domain: domain,
            token_ttl_secs: 3600,
        }
    }

    /// Create a provider with a custom token TTL (useful for testing).
    pub fn with_ttl(key_material: &[u8], domain: ClusterDomain, token_ttl_secs: i64) -> Self {
        Self {
            signing_key: hmac::Key::new(hmac::HMAC_SHA256, key_material),
            key_id: "picloud-hmac-1".to_string(),
            identities: Arc::new(RwLock::new(HashMap::new())),
            app_registrations: Arc::new(RwLock::new(HashMap::new())),
            _iri_builder: IriBuilder::new(domain.clone()),
            cluster_domain: domain,
            token_ttl_secs,
        }
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

        let now = Utc::now();
        let claims = TokenClaims {
            identity_iri: identity_iri.as_str().to_string(),
            product: product.map(|s| s.to_string()),
            roles,
            issued_at: now.timestamp(),
            expires_at: (now + Duration::seconds(self.token_ttl_secs)).timestamp(),
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

        // Issue a token scoped to the product
        let identity_iri = app.registration.product_iri.clone();
        let token = self
            .issue_token(&identity_iri, Some(&app.product_name))
            .await?;

        Ok(TokenResponse {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_in: self.token_ttl_secs,
            scope: scope.map(|s| s.to_string()),
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
}
