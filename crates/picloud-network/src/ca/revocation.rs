//! Certificate Revocation List — in-memory CRL replicated via Raft (ADR-053).
//!
//! When a node is removed (`picloud node remove`), its certificate is
//! added to the CRL. All nodes refresh the CRL from Raft state, and
//! the revoked node's mTLS connections are rejected within one Raft heartbeat.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

/// Reason for certificate revocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RevocationReason {
    /// Node was removed from the cluster.
    NodeRemoved,
    /// Private key was compromised.
    KeyCompromise,
    /// Certificate was replaced by a newer one.
    Superseded,
    /// Administrative revocation.
    Admin,
}

/// A revoked certificate entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokedCert {
    /// SHA-256 fingerprint of the revoked certificate.
    pub fingerprint: String,
    /// Node ID that owned the certificate (if applicable).
    pub node_id: Option<Uuid>,
    /// When the certificate was revoked.
    pub revoked_at: DateTime<Utc>,
    /// Reason for revocation.
    pub reason: RevocationReason,
}

/// In-memory Certificate Revocation List.
///
/// Stored in Raft state and replicated to all nodes.
/// The `fingerprint_set` provides O(1) lookup for the mTLS hot path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RevocationList {
    revoked: Vec<RevokedCert>,
}

impl RevocationList {
    pub fn new() -> Self {
        Self { revoked: Vec::new() }
    }

    /// Add a certificate to the revocation list.
    pub fn revoke(&mut self, entry: RevokedCert) {
        info!(
            fingerprint = %entry.fingerprint,
            reason = ?entry.reason,
            "Certificate revoked"
        );
        self.revoked.push(entry);
    }

    /// Check if a certificate fingerprint is revoked.
    pub fn is_revoked(&self, fingerprint: &str) -> bool {
        self.revoked.iter().any(|r| r.fingerprint == fingerprint)
    }

    /// Return a HashSet of revoked fingerprints for O(1) lookup in the mTLS hot path.
    pub fn fingerprint_set(&self) -> HashSet<String> {
        self.revoked.iter().map(|r| r.fingerprint.clone()).collect()
    }

    /// Get all revoked entries.
    pub fn entries(&self) -> &[RevokedCert] {
        &self.revoked
    }

    /// Number of revoked certificates.
    pub fn len(&self) -> usize {
        self.revoked.len()
    }

    pub fn is_empty(&self) -> bool {
        self.revoked.is_empty()
    }
}

/// Handle a NodeRemoved event — revoke the node's certificate.
pub fn handle_node_removed(
    crl: &mut RevocationList,
    node_id: Uuid,
    fingerprint: &str,
) {
    crl.revoke(RevokedCert {
        fingerprint: fingerprint.to_string(),
        node_id: Some(node_id),
        revoked_at: Utc::now(),
        reason: RevocationReason::NodeRemoved,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revocation_list_starts_empty() {
        let crl = RevocationList::new();
        assert!(crl.is_empty());
        assert_eq!(crl.len(), 0);
    }

    #[test]
    fn revoke_and_check() {
        let mut crl = RevocationList::new();
        crl.revoke(RevokedCert {
            fingerprint: "sha256:abc123".to_string(),
            node_id: Some(Uuid::new_v4()),
            revoked_at: Utc::now(),
            reason: RevocationReason::NodeRemoved,
        });
        assert!(crl.is_revoked("sha256:abc123"));
        assert!(!crl.is_revoked("sha256:other"));
        assert_eq!(crl.len(), 1);
    }

    #[test]
    fn fingerprint_set_for_hot_path() {
        let mut crl = RevocationList::new();
        crl.revoke(RevokedCert {
            fingerprint: "sha256:aaa".to_string(),
            node_id: None,
            revoked_at: Utc::now(),
            reason: RevocationReason::Admin,
        });
        crl.revoke(RevokedCert {
            fingerprint: "sha256:bbb".to_string(),
            node_id: None,
            revoked_at: Utc::now(),
            reason: RevocationReason::KeyCompromise,
        });
        let set = crl.fingerprint_set();
        assert!(set.contains("sha256:aaa"));
        assert!(set.contains("sha256:bbb"));
        assert!(!set.contains("sha256:ccc"));
    }

    #[test]
    fn handle_node_removed_adds_to_crl() {
        let mut crl = RevocationList::new();
        let node_id = Uuid::new_v4();
        handle_node_removed(&mut crl, node_id, "sha256:node_fp");
        assert!(crl.is_revoked("sha256:node_fp"));
        assert_eq!(crl.entries()[0].reason, RevocationReason::NodeRemoved);
    }
}
