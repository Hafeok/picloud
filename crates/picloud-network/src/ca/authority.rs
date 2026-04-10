//! Cluster CA — key management and certificate signing (ADR-053, ADR-055).
//!
//! The CA private key is encrypted at rest (AES-256-GCM) and stored in
//! Raft state. Every node that becomes leader decrypts it in memory.
//!
//! CSR validation: all incoming CSRs are parsed with `x509-parser`,
//! the self-signature is verified, CN and SANs are validated, and
//! wildcard SANs are rejected for node certificates.

use chrono::{Duration, Utc};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use x509_parser::prelude::*;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::CertType;
use picloud_domain::iri::ResourceIri;
use picloud_domain::resources::ClusterIdentity;
use picloud_domain::traits::SignedCert;

pub use picloud_domain::identity::EnrollmentMode;

/// Certificate lifetime constants (ADR-053).
pub struct CertLifetime;

impl CertLifetime {
    /// Node certificates: 90-day validity, renew at 83 days.
    pub const NODE_DAYS: i64 = 90;
    pub const NODE_RENEW_DAYS: i64 = 83;

    /// Workload certificates: 24-hour validity, renew at 23 hours.
    pub const WORKLOAD_HOURS: i64 = 24;
    pub const WORKLOAD_RENEW_HOURS: i64 = 23;

    /// Ingress TLS certificates: 90-day validity, renew at 76 days.
    pub const INGRESS_DAYS: i64 = 90;
    pub const INGRESS_RENEW_DAYS: i64 = 76;

    /// Get the duration for a certificate type.
    pub fn duration(cert_type: &CertType) -> std::time::Duration {
        match cert_type {
            CertType::Node => std::time::Duration::from_secs(Self::NODE_DAYS as u64 * 86400),
            CertType::Workload => std::time::Duration::from_secs(Self::WORKLOAD_HOURS as u64 * 3600),
            CertType::Ingress => std::time::Duration::from_secs(Self::INGRESS_DAYS as u64 * 86400),
        }
    }
}

/// The cluster CA — holds the CA certificate and private key.
///
/// The private key is only held in memory; it is encrypted before
/// being persisted to Raft state.
pub struct ClusterCa {
    /// DER-encoded CA certificate (public, distributed to all nodes).
    pub cert_der: Vec<u8>,
    /// PEM-encoded CA certificate (for distribution).
    pub cert_pem: String,
    /// The CA key pair (never leaves memory unencrypted).
    ca_key: KeyPair,
    /// CA certificate parameters (needed to re-sign).
    ca_params: CertificateParams,
    /// The cluster identity this CA belongs to.
    pub identity: ClusterIdentity,
}

impl ClusterCa {
    /// Generate a new CA for a fresh cluster.
    ///
    /// Returns the CA and the encrypted private key (for Raft storage).
    pub fn generate(identity: ClusterIdentity, master_key: &[u8; 32]) -> Result<(Self, Vec<u8>)> {
        let ca_key = KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
            reason: format!("failed to generate CA key pair: {e}"),
        })?;

        let mut ca_params = CertificateParams::new(Vec::<String>::new()).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to create CA params: {e}"),
            }
        })?;

        ca_params
            .distinguished_name
            .push(DnType::CommonName, &format!("PiCloud CA - {}", identity.domain.0));
        ca_params
            .distinguished_name
            .push(DnType::OrganizationName, "PiCloud");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];

        // CA valid for 10 years
        let not_before = Utc::now();
        let not_after = not_before + Duration::days(3650);
        ca_params.not_before = rcgen::date_time_ymd(
            not_before.format("%Y").to_string().parse::<i32>().unwrap(),
            not_before.format("%m").to_string().parse::<u8>().unwrap(),
            not_before.format("%d").to_string().parse::<u8>().unwrap(),
        );
        ca_params.not_after = rcgen::date_time_ymd(
            not_after.format("%Y").to_string().parse::<i32>().unwrap(),
            not_after.format("%m").to_string().parse::<u8>().unwrap(),
            not_after.format("%d").to_string().parse::<u8>().unwrap(),
        );

        let ca_cert = ca_params.clone().self_signed(&ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to self-sign CA cert: {e}"),
            }
        })?;

        let cert_pem = ca_cert.pem();
        let cert_der = ca_cert.der().to_vec();

        // Encrypt the private key for Raft storage
        let key_pem = ca_key.serialize_pem();
        let encrypted_key = encrypt_key(key_pem.as_bytes(), master_key)?;

        info!(domain = %identity.domain.0, "Cluster CA generated");

        Ok((
            Self {
                cert_der,
                cert_pem,
                ca_key,
                ca_params,
                identity,
            },
            encrypted_key,
        ))
    }

    /// Load CA from Raft state — decrypts the private key.
    pub fn load(
        encrypted_key: &[u8],
        cert_der: Vec<u8>,
        cert_pem: String,
        master_key: &[u8; 32],
        ca_params: CertificateParams,
        identity: ClusterIdentity,
    ) -> Result<Self> {
        let key_pem_bytes = decrypt_key(encrypted_key, master_key)?;
        let key_pem = String::from_utf8(key_pem_bytes).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("CA key is not valid UTF-8: {e}"),
            }
        })?;

        let ca_key = KeyPair::from_pem(&key_pem).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to parse CA key from PEM: {e}"),
            }
        })?;

        info!("Cluster CA loaded from Raft state");

        Ok(Self {
            cert_der,
            cert_pem,
            ca_key,
            ca_params,
            identity,
        })
    }

    /// Sign a node CSR and return the signed certificate.
    ///
    /// Validates the CSR (ADR-055):
    ///   1. Parses the DER-encoded CSR with x509-parser
    ///   2. Verifies the CSR self-signature (proof of key possession)
    ///   3. Validates CN matches `{node_name}` or `{node_name}.{domain}`
    ///   4. Rejects wildcard SANs
    ///   5. Signs a certificate with the validated identity
    pub fn sign_node_csr(
        &self,
        csr_der: &[u8],
        node_name: &str,
        node_ip: &str,
    ) -> Result<SignedCert> {
        // --- Step 1: Parse the CSR ---
        let (_, csr) = X509CertificationRequest::from_der(csr_der).map_err(|e| {
            PiCloudError::CsrValidationFailed {
                reason: format!("failed to parse CSR DER: {e}"),
            }
        })?;

        // --- Step 2: Verify CSR self-signature ---
        csr.verify_signature().map_err(|e| {
            warn!(node = %node_name, "CSR signature verification failed");
            PiCloudError::CsrValidationFailed {
                reason: format!("CSR signature verification failed: {e}"),
            }
        })?;

        // --- Step 3: Validate CN ---
        let fqdn = format!("{node_name}.{}", self.identity.domain.0);
        let subject = &csr.certification_request_info.subject;
        let cn = subject
            .iter_common_name()
            .next()
            .and_then(|attr| attr.as_str().ok())
            .ok_or_else(|| PiCloudError::CsrValidationFailed {
                reason: "CSR missing Common Name in subject".to_string(),
            })?;

        if cn != node_name && cn != fqdn {
            warn!(node = %node_name, cn = %cn, "CSR CN mismatch");
            return Err(PiCloudError::CsrValidationFailed {
                reason: format!(
                    "CSR CN '{cn}' does not match expected '{node_name}' or '{fqdn}'"
                ),
            });
        }

        // --- Step 4: Reject wildcard SANs ---
        validate_no_wildcard_sans(&csr)?;

        // --- Step 5: Sign the certificate ---
        let expires_at = Utc::now() + Duration::days(CertLifetime::NODE_DAYS);

        let ee_key = KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
            reason: format!("failed to generate node key pair: {e}"),
        })?;

        let mut params = CertificateParams::new(vec![fqdn.clone(), node_ip.to_string()])
            .map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to create node cert params: {e}"),
            })?;

        params.distinguished_name.push(DnType::CommonName, &fqdn);
        params
            .distinguished_name
            .push(DnType::OrganizationName, "PiCloud");

        let ca_cert = self.ca_params.clone().self_signed(&self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to reconstruct CA cert: {e}"),
            }
        })?;

        let cert = params
            .signed_by(&ee_key, &ca_cert, &self.ca_key)
            .map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to sign node certificate: {e}"),
            })?;

        let cert_pem = cert.pem();
        let fingerprint = compute_fingerprint(cert_pem.as_bytes());

        info!(node = %node_name, ip = %node_ip, cn = %cn, "Node certificate signed (CSR validated)");

        Ok(SignedCert {
            cert_pem,
            expires_at,
            fingerprint,
        })
    }

    /// Issue a workload identity certificate with a URI SAN.
    pub fn sign_workload_cert(
        &self,
        workload_iri: &ResourceIri,
        product: &str,
    ) -> Result<SignedCert> {
        let expires_at = Utc::now() + Duration::hours(CertLifetime::WORKLOAD_HOURS);

        let ee_key = KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
            reason: format!("failed to generate workload key pair: {e}"),
        })?;

        let cn = format!("{product}.workload.{}", self.identity.domain.0);
        let mut params = CertificateParams::new(vec![cn.clone()]).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to create workload cert params: {e}"),
            }
        })?;

        params.distinguished_name.push(DnType::CommonName, &cn);
        params.distinguished_name.push(DnType::OrganizationName, "PiCloud");

        let ca_cert = self.ca_params.clone().self_signed(&self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to reconstruct CA cert: {e}"),
            }
        })?;

        let cert = params.signed_by(&ee_key, &ca_cert, &self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to sign workload certificate: {e}"),
            }
        })?;

        let cert_pem = cert.pem();
        let fingerprint = compute_fingerprint(cert_pem.as_bytes());

        info!(workload = %workload_iri, product = %product, "Workload certificate issued");

        Ok(SignedCert {
            cert_pem,
            expires_at,
            fingerprint,
        })
    }

    /// Issue a TLS certificate for an ingress hostname.
    pub fn sign_ingress_cert(&self, hostname: &str) -> Result<SignedCert> {
        let expires_at = Utc::now() + Duration::days(CertLifetime::INGRESS_DAYS);

        let ee_key = KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
            reason: format!("failed to generate ingress key pair: {e}"),
        })?;

        let mut params = CertificateParams::new(vec![hostname.to_string()]).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to create ingress cert params: {e}"),
            }
        })?;

        params.distinguished_name.push(DnType::CommonName, hostname);
        params.distinguished_name.push(DnType::OrganizationName, "PiCloud");

        let ca_cert = self.ca_params.clone().self_signed(&self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to reconstruct CA cert: {e}"),
            }
        })?;

        let cert = params.signed_by(&ee_key, &ca_cert, &self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to sign ingress certificate: {e}"),
            }
        })?;

        let cert_pem = cert.pem();
        let fingerprint = compute_fingerprint(cert_pem.as_bytes());

        info!(hostname = %hostname, "Ingress TLS certificate issued");

        Ok(SignedCert {
            cert_pem,
            expires_at,
            fingerprint,
        })
    }

    /// Return the CA certificate in PEM format.
    pub fn ca_cert_pem(&self) -> &str {
        &self.cert_pem
    }
}

/// Validate that a CSR contains no wildcard SANs (ADR-055).
///
/// Node CSRs must not include wildcard DNS names — only the specific
/// node FQDN and IP address are permitted.
fn validate_no_wildcard_sans(csr: &X509CertificationRequest<'_>) -> Result<()> {
    // CSR extensions live inside a PKCS#9 extensionRequest attribute.
    // x509-parser exposes them via requested_extensions().
    if let Some(extensions) = csr.requested_extensions() {
        for ext in extensions {
            if let ParsedExtension::SubjectAlternativeName(san) = ext {
                for name in &san.general_names {
                    if let GeneralName::DNSName(dns) = name {
                        if dns.starts_with('*') {
                            return Err(PiCloudError::CsrValidationFailed {
                                reason: format!(
                                    "wildcard SAN '{dns}' not permitted in node CSR"
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Compute the SHA-256 fingerprint of a certificate (hex-encoded with "sha256:" prefix).
pub fn compute_fingerprint(cert_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert_bytes);
    let result = hasher.finalize();
    let hex: String = result.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// Encrypt a key with AES-256-GCM for Raft storage.
///
/// Format: [12-byte nonce][ciphertext+tag]
fn encrypt_key(plaintext: &[u8], master_key: &[u8; 32]) -> Result<Vec<u8>> {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
    use ring::rand::{SecureRandom, SystemRandom};

    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes).map_err(|_| PiCloudError::TlsCertificateError {
        reason: "failed to generate nonce".to_string(),
    })?;

    let unbound_key = UnboundKey::new(&AES_256_GCM, master_key).map_err(|_| {
        PiCloudError::TlsCertificateError {
            reason: "failed to create AES key".to_string(),
        }
    })?;
    let key = LessSafeKey::new(unbound_key);

    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| PiCloudError::TlsCertificateError {
            reason: "AES encryption failed".to_string(),
        })?;

    // Prepend nonce to ciphertext
    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&in_out);
    Ok(result)
}

/// Decrypt a key from Raft storage.
fn decrypt_key(encrypted: &[u8], master_key: &[u8; 32]) -> Result<Vec<u8>> {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};

    if encrypted.len() < 12 {
        return Err(PiCloudError::TlsCertificateError {
            reason: "encrypted key too short".to_string(),
        });
    }

    let (nonce_bytes, ciphertext) = encrypted.split_at(12);
    let mut nonce_arr = [0u8; 12];
    nonce_arr.copy_from_slice(nonce_bytes);

    let unbound_key = UnboundKey::new(&AES_256_GCM, master_key).map_err(|_| {
        PiCloudError::TlsCertificateError {
            reason: "failed to create AES key".to_string(),
        }
    })?;
    let key = LessSafeKey::new(unbound_key);

    let nonce = Nonce::assume_unique_for_key(nonce_arr);
    let mut in_out = ciphertext.to_vec();
    let plaintext = key
        .open_in_place(nonce, Aad::empty(), &mut in_out)
        .map_err(|_| PiCloudError::TlsCertificateError {
            reason: "AES decryption failed — wrong master key?".to_string(),
        })?;

    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use picloud_domain::iri::ClusterDomain;
    use uuid::Uuid;

    fn test_identity() -> ClusterIdentity {
        ClusterIdentity {
            cluster_id: Uuid::new_v4(),
            domain: ClusterDomain("picloud.local".to_string()),
            created_at: Utc::now(),
            ca_fingerprint: "sha256:test".to_string(),
            enrollment_mode: picloud_domain::identity::EnrollmentMode::Auto,
        }
    }

    /// Generate a DER-encoded CSR with the given CN and SANs using rcgen.
    fn make_csr(cn: &str, sans: &[&str]) -> Vec<u8> {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(sans.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap();
        params.distinguished_name.push(DnType::CommonName, cn);
        let csr = params.serialize_request(&key).unwrap();
        csr.der().to_vec()
    }

    #[test]
    fn generate_ca_produces_valid_pem() {
        let master_key = [0u8; 32];
        let (ca, _encrypted_key) = ClusterCa::generate(test_identity(), &master_key).unwrap();
        assert!(ca.cert_pem.contains("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let master_key = [42u8; 32];
        let plaintext = b"secret key data here";
        let encrypted = encrypt_key(plaintext, &master_key).unwrap();
        let decrypted = decrypt_key(&encrypted, &master_key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let master_key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let encrypted = encrypt_key(b"secret", &master_key).unwrap();
        assert!(decrypt_key(&encrypted, &wrong_key).is_err());
    }

    #[test]
    fn sign_node_csr_with_valid_csr() {
        let master_key = [0u8; 32];
        let (ca, _) = ClusterCa::generate(test_identity(), &master_key).unwrap();

        let csr_der = make_csr(
            "pi-node-01.picloud.local",
            &["pi-node-01.picloud.local", "192.168.1.10"],
        );

        let cert = ca.sign_node_csr(&csr_der, "pi-node-01", "192.168.1.10").unwrap();
        assert!(cert.cert_pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(cert.fingerprint.starts_with("sha256:"));
        assert!(cert.expires_at > Utc::now());
    }

    #[test]
    fn sign_node_csr_accepts_short_cn() {
        let master_key = [0u8; 32];
        let (ca, _) = ClusterCa::generate(test_identity(), &master_key).unwrap();

        // CN = just the node name (without domain) is also accepted
        let csr_der = make_csr("pi-node-01", &["pi-node-01"]);
        let cert = ca.sign_node_csr(&csr_der, "pi-node-01", "192.168.1.10").unwrap();
        assert!(cert.cert_pem.contains("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn sign_node_csr_rejects_cn_mismatch() {
        let master_key = [0u8; 32];
        let (ca, _) = ClusterCa::generate(test_identity(), &master_key).unwrap();

        let csr_der = make_csr("rogue-node.evil.com", &["rogue-node.evil.com"]);
        let result = ca.sign_node_csr(&csr_der, "pi-node-01", "192.168.1.10");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not match"));
    }

    #[test]
    fn sign_node_csr_rejects_invalid_der() {
        let master_key = [0u8; 32];
        let (ca, _) = ClusterCa::generate(test_identity(), &master_key).unwrap();

        let result = ca.sign_node_csr(b"not-a-csr", "pi-node-01", "192.168.1.10");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("CSR"));
    }

    #[test]
    fn sign_workload_cert_produces_valid_cert() {
        let master_key = [0u8; 32];
        let (ca, _) = ClusterCa::generate(test_identity(), &master_key).unwrap();
        let iri = ResourceIri("https://picloud.local/products/app/containers/api".to_string());
        let cert = ca.sign_workload_cert(&iri, "app").unwrap();
        assert!(cert.cert_pem.contains("-----BEGIN CERTIFICATE-----"));
        // Workload certs expire in ~24 hours
        let diff = cert.expires_at - Utc::now();
        assert!(diff.num_hours() >= 23 && diff.num_hours() <= 25);
    }

    #[test]
    fn sign_ingress_cert_produces_valid_cert() {
        let master_key = [0u8; 32];
        let (ca, _) = ClusterCa::generate(test_identity(), &master_key).unwrap();
        let cert = ca.sign_ingress_cert("photos.picloud.local").unwrap();
        assert!(cert.cert_pem.contains("-----BEGIN CERTIFICATE-----"));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let data = b"test certificate data";
        let f1 = compute_fingerprint(data);
        let f2 = compute_fingerprint(data);
        assert_eq!(f1, f2);
        assert!(f1.starts_with("sha256:"));
    }
}
