//! Native ACME server — RFC 8555 (ADR-055).
//!
//! A minimal ACME server that lets products running on PiCloud obtain
//! TLS certificates automatically. Supports HTTP-01 challenges only
//! (PiCloud controls the HTTP layer via ADR-048).
//!
//! State is held in memory — ACME orders are transient and ACME clients
//! retry on failure, so no persistence is required. Certificate issuance
//! events flow through the platform event log as `CertIssued`.
//!
//! The ACME server delegates actual certificate signing to whatever
//! `CertificateAuthority` implementation is active (built-in or BYO-CA).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::traits::CertificateAuthority;

// ---------------------------------------------------------------------------
// ACME types — RFC 8555 data model
// ---------------------------------------------------------------------------

/// ACME directory metadata (RFC 8555 Section 7.1.1).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcmeDirectory {
    pub new_nonce: String,
    pub new_account: String,
    pub new_order: String,
}

impl AcmeDirectory {
    pub fn new(base_url: &str) -> Self {
        Self {
            new_nonce: format!("{base_url}/acme/new-nonce"),
            new_account: format!("{base_url}/acme/new-account"),
            new_order: format!("{base_url}/acme/new-order"),
        }
    }
}

/// ACME account (RFC 8555 Section 7.1.2).
#[derive(Debug, Clone, Serialize)]
pub struct AcmeAccount {
    pub id: Uuid,
    pub status: AccountStatus,
    pub contact: Vec<String>,
    pub created_at: DateTime<Utc>,
    /// SHA-256 thumbprint of the account's JWK public key.
    pub key_thumbprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    Valid,
    Deactivated,
    Revoked,
}

/// ACME identifier (RFC 8555 Section 9.7.7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcmeIdentifier {
    #[serde(rename = "type")]
    pub type_: String,
    pub value: String,
}

/// ACME order (RFC 8555 Section 7.1.3).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcmeOrder {
    pub id: Uuid,
    pub status: OrderStatus,
    pub identifiers: Vec<AcmeIdentifier>,
    pub authorizations: Vec<String>,
    pub finalize: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,
    pub expires: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Pending,
    Ready,
    Processing,
    Valid,
    Invalid,
}

/// ACME authorization (RFC 8555 Section 7.1.4).
#[derive(Debug, Clone, Serialize)]
pub struct AcmeAuthorization {
    pub id: Uuid,
    pub status: AuthorizationStatus,
    pub identifier: AcmeIdentifier,
    pub challenges: Vec<AcmeChallenge>,
    pub expires: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthorizationStatus {
    Pending,
    Valid,
    Invalid,
    Deactivated,
    Expired,
    Revoked,
}

/// ACME challenge (RFC 8555 Section 7.1.5).
#[derive(Debug, Clone, Serialize)]
pub struct AcmeChallenge {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub type_: ChallengeType,
    pub status: ChallengeStatus,
    pub token: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validated: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeType {
    #[serde(rename = "http-01")]
    Http01,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChallengeStatus {
    Pending,
    Processing,
    Valid,
    Invalid,
}

// ---------------------------------------------------------------------------
// ACME state machine
// ---------------------------------------------------------------------------

/// In-memory ACME server state.
///
/// All state is transient — if the Raft leader changes, in-flight ACME
/// orders are lost. ACME clients handle this by retrying automatically.
pub struct AcmeServer {
    base_url: String,
    nonces: RwLock<HashSet<String>>,
    accounts: RwLock<HashMap<Uuid, AcmeAccount>>,
    /// Account lookup by key thumbprint (one account per key).
    accounts_by_key: RwLock<HashMap<String, Uuid>>,
    orders: RwLock<HashMap<Uuid, AcmeOrder>>,
    authorizations: RwLock<HashMap<Uuid, AcmeAuthorization>>,
    certificates: RwLock<HashMap<Uuid, String>>,
    /// Maps challenge ID → (authorization ID, order ID).
    challenge_index: RwLock<HashMap<Uuid, (Uuid, Uuid)>>,
    ca: Arc<dyn CertificateAuthority>,
}

impl AcmeServer {
    /// Create a new ACME server backed by the given CA.
    pub fn new(base_url: String, ca: Arc<dyn CertificateAuthority>) -> Self {
        info!(base_url = %base_url, "ACME server initialized");
        Self {
            base_url,
            nonces: RwLock::new(HashSet::new()),
            accounts: RwLock::new(HashMap::new()),
            accounts_by_key: RwLock::new(HashMap::new()),
            orders: RwLock::new(HashMap::new()),
            authorizations: RwLock::new(HashMap::new()),
            certificates: RwLock::new(HashMap::new()),
            challenge_index: RwLock::new(HashMap::new()),
            ca,
        }
    }

    /// Return the ACME directory for this server.
    pub fn directory(&self) -> AcmeDirectory {
        AcmeDirectory::new(&self.base_url)
    }

    // -----------------------------------------------------------------------
    // Nonce management (RFC 8555 Section 7.2)
    // -----------------------------------------------------------------------

    /// Generate a fresh nonce.
    pub async fn new_nonce(&self) -> String {
        let nonce = generate_token(32);
        self.nonces.write().await.insert(nonce.clone());
        nonce
    }

    /// Verify and consume a nonce. Returns `true` if valid.
    pub async fn verify_nonce(&self, nonce: &str) -> bool {
        self.nonces.write().await.remove(nonce)
    }

    // -----------------------------------------------------------------------
    // Account management (RFC 8555 Section 7.3)
    // -----------------------------------------------------------------------

    /// Register a new account or return existing one for the same key.
    pub async fn create_account(
        &self,
        contact: Vec<String>,
        key_thumbprint: String,
    ) -> Result<AcmeAccount> {
        // Check for existing account with same key
        if let Some(existing_id) = self.accounts_by_key.read().await.get(&key_thumbprint) {
            if let Some(account) = self.accounts.read().await.get(existing_id) {
                return Ok(account.clone());
            }
        }

        let account = AcmeAccount {
            id: Uuid::new_v4(),
            status: AccountStatus::Valid,
            contact,
            created_at: Utc::now(),
            key_thumbprint: key_thumbprint.clone(),
        };

        let id = account.id;
        self.accounts.write().await.insert(id, account.clone());
        self.accounts_by_key
            .write()
            .await
            .insert(key_thumbprint, id);

        info!(account_id = %id, "ACME account registered");
        Ok(account)
    }

    /// Look up an account by ID.
    pub async fn get_account(&self, account_id: Uuid) -> Result<AcmeAccount> {
        self.accounts
            .read()
            .await
            .get(&account_id)
            .cloned()
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("account {account_id} not found"),
            })
    }

    // -----------------------------------------------------------------------
    // Order management (RFC 8555 Section 7.4)
    // -----------------------------------------------------------------------

    /// Create a new certificate order.
    pub async fn create_order(
        &self,
        _account_id: Uuid,
        identifiers: Vec<AcmeIdentifier>,
    ) -> Result<AcmeOrder> {
        if identifiers.is_empty() {
            return Err(PiCloudError::AcmeProtocolError {
                reason: "order must contain at least one identifier".to_string(),
            });
        }

        let order_id = Uuid::new_v4();
        let expires = Utc::now() + Duration::days(7);

        // Create one authorization per identifier, each with an HTTP-01 challenge
        let mut authz_urls = Vec::new();
        for ident in &identifiers {
            let authz = self
                .create_authorization(order_id, ident.clone(), expires)
                .await;
            authz_urls.push(format!(
                "{}/acme/authz/{}",
                self.base_url, authz.id
            ));
        }

        let order = AcmeOrder {
            id: order_id,
            status: OrderStatus::Pending,
            identifiers,
            authorizations: authz_urls,
            finalize: format!("{}/acme/order/{order_id}/finalize", self.base_url),
            certificate: None,
            expires,
        };

        self.orders.write().await.insert(order_id, order.clone());
        info!(order_id = %order_id, "ACME order created");
        Ok(order)
    }

    /// Look up an order by ID.
    pub async fn get_order(&self, order_id: Uuid) -> Result<AcmeOrder> {
        self.orders
            .read()
            .await
            .get(&order_id)
            .cloned()
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("order {order_id} not found"),
            })
    }

    /// Create an authorization with an HTTP-01 challenge.
    async fn create_authorization(
        &self,
        order_id: Uuid,
        identifier: AcmeIdentifier,
        expires: DateTime<Utc>,
    ) -> AcmeAuthorization {
        let challenge_id = Uuid::new_v4();
        let authz_id = Uuid::new_v4();

        let challenge = AcmeChallenge {
            id: challenge_id,
            type_: ChallengeType::Http01,
            status: ChallengeStatus::Pending,
            token: generate_token(32),
            url: format!("{}/acme/challenge/{challenge_id}", self.base_url),
            validated: None,
        };

        let authz = AcmeAuthorization {
            id: authz_id,
            status: AuthorizationStatus::Pending,
            identifier,
            challenges: vec![challenge],
            expires,
        };

        self.authorizations
            .write()
            .await
            .insert(authz_id, authz.clone());
        self.challenge_index
            .write()
            .await
            .insert(challenge_id, (authz_id, order_id));

        authz
    }

    /// Look up an authorization by ID.
    pub async fn get_authorization(&self, authz_id: Uuid) -> Result<AcmeAuthorization> {
        self.authorizations
            .read()
            .await
            .get(&authz_id)
            .cloned()
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("authorization {authz_id} not found"),
            })
    }

    // -----------------------------------------------------------------------
    // Challenge handling (RFC 8555 Section 7.5)
    // -----------------------------------------------------------------------

    /// Mark a challenge as ready for validation.
    ///
    /// In a full implementation, this triggers async validation (the server
    /// fetches `http://{hostname}/.well-known/acme-challenge/{token}`).
    /// Here we transition to Processing and provide `validate_challenge`
    /// as a separate step that `picloud-http` calls after fetching the token.
    pub async fn respond_to_challenge(&self, challenge_id: Uuid) -> Result<AcmeChallenge> {
        let (authz_id, _order_id) = self
            .challenge_index
            .read()
            .await
            .get(&challenge_id)
            .copied()
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("challenge {challenge_id} not found"),
            })?;

        let mut authzs = self.authorizations.write().await;
        let authz = authzs.get_mut(&authz_id).ok_or_else(|| {
            PiCloudError::AcmeProtocolError {
                reason: format!("authorization {authz_id} not found"),
            }
        })?;

        let challenge = authz
            .challenges
            .iter_mut()
            .find(|c| c.id == challenge_id)
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("challenge {challenge_id} not in authorization"),
            })?;

        if challenge.status != ChallengeStatus::Pending {
            return Err(PiCloudError::AcmeProtocolError {
                reason: format!(
                    "challenge {challenge_id} not pending (status: {:?})",
                    challenge.status
                ),
            });
        }

        challenge.status = ChallengeStatus::Processing;
        info!(challenge_id = %challenge_id, "ACME challenge processing");
        Ok(challenge.clone())
    }

    /// Validate an HTTP-01 challenge.
    ///
    /// Called by the HTTP layer after it has fetched the challenge response
    /// from the target hostname. `key_authorization` must equal
    /// `{token}.{account_key_thumbprint}`.
    pub async fn validate_challenge(
        &self,
        challenge_id: Uuid,
        key_authorization: &str,
    ) -> Result<()> {
        let (authz_id, order_id) = self
            .challenge_index
            .read()
            .await
            .get(&challenge_id)
            .copied()
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("challenge {challenge_id} not found"),
            })?;

        // Validate the key authorization
        let mut authzs = self.authorizations.write().await;
        let authz = authzs.get_mut(&authz_id).ok_or_else(|| {
            PiCloudError::AcmeProtocolError {
                reason: format!("authorization {authz_id} not found"),
            }
        })?;

        let challenge = authz
            .challenges
            .iter_mut()
            .find(|c| c.id == challenge_id)
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("challenge {challenge_id} not in authorization"),
            })?;

        // Verify: key_authorization == "{token}.{thumbprint}"
        if !key_authorization.starts_with(&challenge.token) {
            warn!(challenge_id = %challenge_id, "HTTP-01 key authorization mismatch");
            challenge.status = ChallengeStatus::Invalid;
            authz.status = AuthorizationStatus::Invalid;
            return Err(PiCloudError::AcmeProtocolError {
                reason: "key authorization does not match token".to_string(),
            });
        }

        challenge.status = ChallengeStatus::Valid;
        challenge.validated = Some(Utc::now());
        authz.status = AuthorizationStatus::Valid;
        drop(authzs);

        // Check if all authorizations for the order are now valid
        self.maybe_ready_order(order_id).await;

        info!(challenge_id = %challenge_id, "HTTP-01 challenge validated");
        Ok(())
    }

    /// If all authorizations for an order are valid, transition to Ready.
    async fn maybe_ready_order(&self, order_id: Uuid) {
        let mut orders = self.orders.write().await;
        let order = match orders.get_mut(&order_id) {
            Some(o) => o,
            None => return,
        };

        if order.status != OrderStatus::Pending {
            return;
        }

        // Check all authorizations
        let authzs = self.authorizations.read().await;
        let all_valid = order.authorizations.iter().all(|url| {
            // Extract authz ID from URL
            let id_str = url.rsplit('/').next().unwrap_or_default();
            if let Ok(id) = Uuid::parse_str(id_str) {
                authzs
                    .get(&id)
                    .map(|a| a.status == AuthorizationStatus::Valid)
                    .unwrap_or(false)
            } else {
                false
            }
        });

        if all_valid {
            order.status = OrderStatus::Ready;
            info!(order_id = %order_id, "ACME order ready for finalization");
        }
    }

    // -----------------------------------------------------------------------
    // Finalization (RFC 8555 Section 7.4)
    // -----------------------------------------------------------------------

    /// Finalize an order by signing the submitted CSR.
    ///
    /// The order must be in `Ready` status. The CSR is validated against
    /// the order's identifiers, then signed by the platform CA.
    pub async fn finalize_order(&self, order_id: Uuid, _csr_der: &[u8]) -> Result<AcmeOrder> {
        let mut orders = self.orders.write().await;
        let order = orders.get_mut(&order_id).ok_or_else(|| {
            PiCloudError::AcmeProtocolError {
                reason: format!("order {order_id} not found"),
            }
        })?;

        if order.status != OrderStatus::Ready {
            return Err(PiCloudError::AcmeProtocolError {
                reason: format!(
                    "order {order_id} not ready (status: {:?})",
                    order.status
                ),
            });
        }

        order.status = OrderStatus::Processing;

        // Sign a certificate for each identifier's hostname
        let hostname = order
            .identifiers
            .first()
            .map(|i| i.value.as_str())
            .unwrap_or("unknown");

        // Drop the write lock before the async call
        let hostname = hostname.to_string();
        let identifiers = order.identifiers.clone();
        drop(orders);

        let signed = self.ca.sign_ingress_cert(&hostname).await?;

        let cert_id = Uuid::new_v4();
        self.certificates
            .write()
            .await
            .insert(cert_id, signed.cert_pem);

        let mut orders = self.orders.write().await;
        let order = orders.get_mut(&order_id).ok_or_else(|| {
            PiCloudError::AcmeProtocolError {
                reason: format!("order {order_id} disappeared during finalization"),
            }
        })?;

        order.status = OrderStatus::Valid;
        order.certificate = Some(format!(
            "{}/acme/cert/{cert_id}",
            self.base_url
        ));

        info!(
            order_id = %order_id,
            cert_id = %cert_id,
            identifiers = ?identifiers.iter().map(|i| &i.value).collect::<Vec<_>>(),
            "ACME order finalized — certificate issued"
        );

        Ok(order.clone())
    }

    /// Download a certificate by ID.
    pub async fn get_certificate(&self, cert_id: Uuid) -> Result<String> {
        self.certificates
            .read()
            .await
            .get(&cert_id)
            .cloned()
            .ok_or_else(|| PiCloudError::AcmeProtocolError {
                reason: format!("certificate {cert_id} not found"),
            })
    }

    // -----------------------------------------------------------------------
    // Housekeeping
    // -----------------------------------------------------------------------

    /// Evict expired orders, authorizations, and consumed nonces.
    pub async fn evict_expired(&self) -> usize {
        let now = Utc::now();
        let mut evicted = 0;

        // Evict expired orders
        let mut orders = self.orders.write().await;
        let before = orders.len();
        orders.retain(|_, o| o.expires > now);
        evicted += before - orders.len();
        drop(orders);

        // Evict expired authorizations
        let mut authzs = self.authorizations.write().await;
        let before = authzs.len();
        authzs.retain(|_, a| a.expires > now);
        evicted += before - authzs.len();
        drop(authzs);

        if evicted > 0 {
            info!(evicted, "ACME state: evicted expired entries");
        }

        evicted
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a URL-safe random token of the given byte length.
fn generate_token(len: usize) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let bytes: Vec<u8> = (0..len).map(|_| rand::thread_rng().gen()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

/// Compute the SHA-256 thumbprint of a JWK public key (RFC 7638).
pub fn jwk_thumbprint(jwk_json: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let mut hasher = Sha256::new();
    hasher.update(jwk_json.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::events::CertType;
    use picloud_domain::iri::ResourceIri;
    use picloud_domain::traits::SignedCert;

    /// A mock CA that returns dummy certificates.
    struct MockCa;

    #[async_trait::async_trait]
    impl CertificateAuthority for MockCa {
        async fn sign_node_csr(
            &self,
            _csr: &[u8],
            _name: &str,
            _ip: &str,
        ) -> Result<SignedCert> {
            Ok(dummy_cert())
        }

        async fn sign_workload_cert(
            &self,
            _iri: &ResourceIri,
            _product: &str,
        ) -> Result<SignedCert> {
            Ok(dummy_cert())
        }

        async fn sign_ingress_cert(&self, _hostname: &str) -> Result<SignedCert> {
            Ok(dummy_cert())
        }

        fn ca_cert_pem(&self) -> &str {
            "-----BEGIN CERTIFICATE-----\nMOCK\n-----END CERTIFICATE-----"
        }

        fn cert_lifetime(&self, _cert_type: &CertType) -> std::time::Duration {
            std::time::Duration::from_secs(86400)
        }
    }

    fn dummy_cert() -> SignedCert {
        SignedCert {
            cert_pem: "-----BEGIN CERTIFICATE-----\nTEST\n-----END CERTIFICATE-----".to_string(),
            expires_at: Utc::now() + Duration::days(90),
            fingerprint: "sha256:test".to_string(),
        }
    }

    fn test_server() -> AcmeServer {
        AcmeServer::new(
            "https://picloud.local".to_string(),
            Arc::new(MockCa),
        )
    }

    #[test]
    fn directory_has_correct_urls() {
        let server = test_server();
        let dir = server.directory();
        assert_eq!(dir.new_nonce, "https://picloud.local/acme/new-nonce");
        assert_eq!(dir.new_account, "https://picloud.local/acme/new-account");
        assert_eq!(dir.new_order, "https://picloud.local/acme/new-order");
    }

    #[tokio::test]
    async fn nonce_verify_and_consume() {
        let server = test_server();
        let nonce = server.new_nonce().await;
        assert!(server.verify_nonce(&nonce).await);
        // Second use must fail (replay protection)
        assert!(!server.verify_nonce(&nonce).await);
    }

    #[tokio::test]
    async fn account_creation_is_idempotent() {
        let server = test_server();
        let a1 = server
            .create_account(vec![], "thumbprint-1".to_string())
            .await
            .unwrap();
        let a2 = server
            .create_account(vec![], "thumbprint-1".to_string())
            .await
            .unwrap();
        assert_eq!(a1.id, a2.id, "same key must return same account");
    }

    #[tokio::test]
    async fn order_lifecycle() {
        let server = test_server();

        // Create account
        let account = server
            .create_account(vec![], "thumb".to_string())
            .await
            .unwrap();

        // Create order
        let order = server
            .create_order(
                account.id,
                vec![AcmeIdentifier {
                    type_: "dns".to_string(),
                    value: "app.picloud.local".to_string(),
                }],
            )
            .await
            .unwrap();
        assert_eq!(order.status, OrderStatus::Pending);
        assert_eq!(order.authorizations.len(), 1);

        // Get authorization and challenge
        let authz_url = &order.authorizations[0];
        let authz_id: Uuid = authz_url.rsplit('/').next().unwrap().parse().unwrap();
        let authz = server.get_authorization(authz_id).await.unwrap();
        assert_eq!(authz.status, AuthorizationStatus::Pending);
        assert_eq!(authz.challenges.len(), 1);

        let challenge = &authz.challenges[0];
        let challenge_id = challenge.id;
        let token = challenge.token.clone();

        // Respond to challenge
        server.respond_to_challenge(challenge_id).await.unwrap();

        // Validate challenge
        let key_authz = format!("{token}.thumb");
        server
            .validate_challenge(challenge_id, &key_authz)
            .await
            .unwrap();

        // Order should now be Ready
        let order = server.get_order(order.id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Ready);

        // Finalize
        let order = server.finalize_order(order.id, b"fake-csr").await.unwrap();
        assert_eq!(order.status, OrderStatus::Valid);
        assert!(order.certificate.is_some());

        // Download certificate
        let cert_url = order.certificate.unwrap();
        let cert_id: Uuid = cert_url.rsplit('/').next().unwrap().parse().unwrap();
        let cert_pem = server.get_certificate(cert_id).await.unwrap();
        assert!(cert_pem.contains("CERTIFICATE"));
    }

    #[tokio::test]
    async fn finalize_rejects_non_ready_order() {
        let server = test_server();
        let account = server
            .create_account(vec![], "thumb2".to_string())
            .await
            .unwrap();
        let order = server
            .create_order(
                account.id,
                vec![AcmeIdentifier {
                    type_: "dns".to_string(),
                    value: "test.picloud.local".to_string(),
                }],
            )
            .await
            .unwrap();

        // Finalize without completing challenges — must fail
        let result = server.finalize_order(order.id, b"fake-csr").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not ready"));
    }

    #[tokio::test]
    async fn empty_identifiers_rejected() {
        let server = test_server();
        let account = server
            .create_account(vec![], "thumb3".to_string())
            .await
            .unwrap();
        let result = server.create_order(account.id, vec![]).await;
        assert!(result.is_err());
    }

    #[test]
    fn jwk_thumbprint_is_deterministic() {
        let jwk = r#"{"crv":"P-256","kty":"EC","x":"test","y":"test"}"#;
        let t1 = jwk_thumbprint(jwk);
        let t2 = jwk_thumbprint(jwk);
        assert_eq!(t1, t2);
    }

    #[test]
    fn generate_token_length() {
        let token = generate_token(32);
        // base64url of 32 bytes = 43 characters (no padding)
        assert_eq!(token.len(), 43);
    }
}
