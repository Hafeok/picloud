//! Node enrollment — /enroll endpoint handler (ADR-053).
//!
//! New nodes POST a CSR to receive a signed certificate.
//! In Auto mode, network presence is authorization.
//! In Token mode, a valid enrollment token is required.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::identity::EnrollmentMode;
use super::authority::ClusterCa;

/// A single-use, time-limited enrollment token (ADR-053).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub id: Uuid,
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    /// If set, this token can only be used by this specific node name.
    pub for_node: Option<String>,
}

impl EnrollmentToken {
    /// Check if this token is still valid (not used and not expired).
    pub fn is_valid(&self) -> bool {
        !self.used && Utc::now() < self.expires_at
    }
}

/// Request from a new node to enroll in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentRequest {
    /// DER-encoded CSR from the new node.
    pub csr_der: Vec<u8>,
    /// Hostname of the new node.
    pub node_name: String,
    /// IP address of the new node.
    pub node_ip: String,
    /// Enrollment token (required in Token mode, ignored in Auto mode).
    pub enrollment_token: Option<String>,
    /// Cluster ID the node expects to join.
    pub cluster_id: Uuid,
}

/// Response sent to a newly enrolled node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentResponse {
    pub node_cert_pem: String,
    pub ca_cert_pem: String,
    pub expires_at: DateTime<Utc>,
    pub fingerprint: String,
}

/// In-memory token store — replicated via Raft in production.
pub struct TokenStore {
    tokens: HashMap<Uuid, EnrollmentToken>,
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Issue a new enrollment token with the given TTL.
    pub fn issue(
        &mut self,
        ttl: Duration,
        created_by: &str,
        for_node: Option<String>,
    ) -> EnrollmentToken {
        let token = EnrollmentToken {
            id: Uuid::new_v4(),
            token: format!("enroll-{}", Uuid::new_v4()),
            expires_at: Utc::now() + ttl,
            used: false,
            created_by: created_by.to_string(),
            created_at: Utc::now(),
            for_node,
        };
        info!(token_id = %token.id, "Enrollment token issued");
        self.tokens.insert(token.id, token.clone());
        token
    }

    /// Consume a token — marks it as used. Returns error if invalid.
    pub fn consume(&mut self, token_str: &str) -> Result<EnrollmentToken> {
        let entry = self
            .tokens
            .values_mut()
            .find(|t| t.token == token_str)
            .ok_or_else(|| PiCloudError::EnrollmentFailed {
                reason: "enrollment token not found".to_string(),
            })?;

        if entry.used {
            return Err(PiCloudError::EnrollmentFailed {
                reason: "enrollment token already used".to_string(),
            });
        }
        if Utc::now() >= entry.expires_at {
            return Err(PiCloudError::EnrollmentFailed {
                reason: "enrollment token expired".to_string(),
            });
        }

        entry.used = true;
        Ok(entry.clone())
    }

    /// List all active (valid) tokens.
    pub fn active_tokens(&self) -> Vec<&EnrollmentToken> {
        self.tokens.values().filter(|t| t.is_valid()).collect()
    }

    /// Revoke a token by ID.
    pub fn revoke(&mut self, token_id: Uuid) -> Result<()> {
        match self.tokens.get_mut(&token_id) {
            Some(token) => {
                token.used = true;
                info!(token_id = %token_id, "Enrollment token revoked");
                Ok(())
            }
            None => Err(PiCloudError::EnrollmentFailed {
                reason: format!("token {token_id} not found"),
            }),
        }
    }

    /// Remove expired tokens from the store.
    pub fn evict_expired(&mut self) -> usize {
        let before = self.tokens.len();
        self.tokens.retain(|_, t| !t.used || Utc::now() < t.expires_at + Duration::hours(1));
        before - self.tokens.len()
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle an enrollment request from a new node.
///
/// Validates the request based on the enrollment mode and signs the CSR.
pub fn handle_enrollment(
    request: &EnrollmentRequest,
    ca: &ClusterCa,
    mode: &EnrollmentMode,
    tokens: &mut TokenStore,
    cluster_id: Uuid,
) -> Result<EnrollmentResponse> {
    // Validate cluster ID
    if request.cluster_id != cluster_id {
        warn!(
            expected = %cluster_id,
            got = %request.cluster_id,
            "Enrollment rejected: cluster ID mismatch"
        );
        return Err(PiCloudError::EnrollmentFailed {
            reason: format!(
                "cluster ID mismatch: expected {cluster_id}, got {}",
                request.cluster_id
            ),
        });
    }

    // Mode-specific authorization
    match mode {
        EnrollmentMode::Auto => {
            info!(
                node = %request.node_name,
                ip = %request.node_ip,
                "Auto-enrollment: accepting node"
            );
        }
        EnrollmentMode::Token => {
            let token_str = request.enrollment_token.as_deref().ok_or_else(|| {
                PiCloudError::EnrollmentFailed {
                    reason: "enrollment token required in Token mode".to_string(),
                }
            })?;

            let token = tokens.consume(token_str)?;

            // If token is for a specific node, verify it matches
            if let Some(ref for_node) = token.for_node {
                if for_node != &request.node_name {
                    return Err(PiCloudError::EnrollmentFailed {
                        reason: format!(
                            "token issued for node '{}', but request is from '{}'",
                            for_node, request.node_name
                        ),
                    });
                }
            }

            info!(
                node = %request.node_name,
                token_id = %token.id,
                "Token enrollment: accepted"
            );
        }
    }

    // Sign the CSR
    let signed = ca.sign_node_csr(&request.csr_der, &request.node_name, &request.node_ip)?;

    Ok(EnrollmentResponse {
        node_cert_pem: signed.cert_pem,
        ca_cert_pem: ca.ca_cert_pem().to_string(),
        expires_at: signed.expires_at,
        fingerprint: signed.fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_valid_when_fresh() {
        let token = EnrollmentToken {
            id: Uuid::new_v4(),
            token: "test".to_string(),
            expires_at: Utc::now() + Duration::hours(2),
            used: false,
            created_by: "admin".to_string(),
            created_at: Utc::now(),
            for_node: None,
        };
        assert!(token.is_valid());
    }

    #[test]
    fn token_invalid_when_used() {
        let token = EnrollmentToken {
            id: Uuid::new_v4(),
            token: "test".to_string(),
            expires_at: Utc::now() + Duration::hours(2),
            used: true,
            created_by: "admin".to_string(),
            created_at: Utc::now(),
            for_node: None,
        };
        assert!(!token.is_valid());
    }

    #[test]
    fn token_invalid_when_expired() {
        let token = EnrollmentToken {
            id: Uuid::new_v4(),
            token: "test".to_string(),
            expires_at: Utc::now() - Duration::hours(1),
            used: false,
            created_by: "admin".to_string(),
            created_at: Utc::now(),
            for_node: None,
        };
        assert!(!token.is_valid());
    }

    #[test]
    fn token_store_issue_and_consume() {
        let mut store = TokenStore::new();
        let token = store.issue(Duration::hours(2), "admin", None);
        assert_eq!(store.active_tokens().len(), 1);

        let consumed = store.consume(&token.token).unwrap();
        assert!(consumed.used);
        assert_eq!(store.active_tokens().len(), 0);
    }

    #[test]
    fn token_store_consume_twice_fails() {
        let mut store = TokenStore::new();
        let token = store.issue(Duration::hours(2), "admin", None);
        store.consume(&token.token).unwrap();
        assert!(store.consume(&token.token).is_err());
    }

    #[test]
    fn token_store_revoke() {
        let mut store = TokenStore::new();
        let token = store.issue(Duration::hours(2), "admin", None);
        store.revoke(token.id).unwrap();
        assert_eq!(store.active_tokens().len(), 0);
    }
}
