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

/// Local identity provider backed by HMAC-SHA256 token signing and rcgen certificates.
pub struct LocalIdentityProvider {
    signing_key: hmac::Key,
    identities: Arc<RwLock<HashMap<String, StoredIdentity>>>,
    _iri_builder: IriBuilder,
    /// Token validity duration in seconds.
    token_ttl_secs: i64,
}

impl LocalIdentityProvider {
    /// Create a new provider with the given HMAC key material.
    pub fn new(key_material: &[u8], domain: ClusterDomain) -> Self {
        Self {
            signing_key: hmac::Key::new(hmac::HMAC_SHA256, key_material),
            identities: Arc::new(RwLock::new(HashMap::new())),
            _iri_builder: IriBuilder::new(domain),
            token_ttl_secs: 3600,
        }
    }

    /// Create a provider with a custom token TTL (useful for testing).
    pub fn with_ttl(key_material: &[u8], domain: ClusterDomain, token_ttl_secs: i64) -> Self {
        Self {
            signing_key: hmac::Key::new(hmac::HMAC_SHA256, key_material),
            identities: Arc::new(RwLock::new(HashMap::new())),
            _iri_builder: IriBuilder::new(domain),
            token_ttl_secs,
        }
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
}
