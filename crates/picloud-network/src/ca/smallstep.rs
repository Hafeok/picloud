//! Smallstep BYO-CA backend — ADR-030, ADR-055.
//!
//! Implements the `CertificateAuthority` trait against an external
//! Smallstep step-ca server's REST API (`/1.0/sign`).
//!
//! Operators who already run step-ca can plug it into PiCloud as their CA:
//! ```bash
//! picloud cluster init --ca-type smallstep \
//!     --ca-endpoint https://step-ca.local:9000 \
//!     --ca-provisioner picloud \
//!     --ca-provisioner-key ./provisioner.key \
//!     --ca-cert ./root_ca.pem
//! ```

use std::time::Duration as StdDuration;

use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::events::CertType;
use picloud_domain::iri::ResourceIri;
use picloud_domain::traits::{CertificateAuthority, SignedCert};

use super::authority::{compute_fingerprint, CertLifetime};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for connecting to an external Smallstep step-ca.
#[derive(Debug, Clone)]
pub struct SmallstepConfig {
    /// Base URL of the step-ca server (e.g. "https://step-ca.local:9000").
    pub endpoint: String,
    /// JWK provisioner name registered in step-ca.
    pub provisioner: String,
    /// Base64url-encoded provisioner signing key (ES256 / P-256 private key).
    pub provisioner_key_b64: String,
    /// CA root certificate PEM — used to verify TLS to step-ca.
    pub ca_cert_pem: String,
    /// SHA-256 fingerprint of the CA root (for verification).
    pub fingerprint: String,
}

// ---------------------------------------------------------------------------
// Smallstep CA backend
// ---------------------------------------------------------------------------

/// A `CertificateAuthority` that delegates signing to an external Smallstep
/// step-ca server. The platform authenticates to step-ca using a JWK
/// provisioner token signed with ES256.
pub struct SmallstepCa {
    config: SmallstepConfig,
    client: reqwest::Client,
}

impl SmallstepCa {
    /// Create a new Smallstep CA backend.
    ///
    /// Builds an HTTP client that trusts the step-ca root certificate.
    pub fn new(config: SmallstepConfig) -> Result<Self> {
        let ca_cert = reqwest::Certificate::from_pem(config.ca_cert_pem.as_bytes()).map_err(
            |e| PiCloudError::ExternalCaError {
                reason: format!("invalid step-ca root certificate PEM: {e}"),
            },
        )?;

        let client = reqwest::Client::builder()
            .add_root_certificate(ca_cert)
            .timeout(StdDuration::from_secs(30))
            .build()
            .map_err(|e| PiCloudError::ExternalCaError {
                reason: format!("failed to build step-ca HTTP client: {e}"),
            })?;

        info!(
            endpoint = %config.endpoint,
            provisioner = %config.provisioner,
            "Smallstep BYO-CA backend initialized"
        );

        Ok(Self { config, client })
    }

    /// Return the configured CA certificate PEM.
    pub fn ca_cert_pem(&self) -> &str {
        &self.config.ca_cert_pem
    }

    /// Create a one-time token (OTT) for the step-ca provisioner.
    ///
    /// The token is a compact JWS (ES256) with claims:
    ///   sub: <subject CN>
    ///   iss: <provisioner name>
    ///   aud: https://<endpoint>/1.0/sign
    ///   sha: <CA fingerprint>
    ///   nbf, iat, exp, jti
    fn create_provisioner_token(&self, subject: &str) -> Result<String> {
        let now = Utc::now();
        let jti = Uuid::new_v4().to_string();

        let header = serde_json::json!({
            "alg": "ES256",
            "typ": "JWT",
            "kid": &self.config.provisioner,
        });

        let claims = serde_json::json!({
            "sub": subject,
            "iss": &self.config.provisioner,
            "aud": format!("{}/1.0/sign", &self.config.endpoint),
            "sha": &self.config.fingerprint,
            "nbf": now.timestamp(),
            "iat": now.timestamp(),
            "exp": (now + Duration::minutes(5)).timestamp(),
            "jti": jti,
        });

        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).map_err(|e| {
            PiCloudError::ExternalCaError {
                reason: format!("failed to serialize JWT header: {e}"),
            }
        })?);

        let claims_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).map_err(|e| {
            PiCloudError::ExternalCaError {
                reason: format!("failed to serialize JWT claims: {e}"),
            }
        })?);

        let signing_input = format!("{header_b64}.{claims_b64}");

        // Sign with the provisioner's ES256 key
        let key_bytes = URL_SAFE_NO_PAD
            .decode(&self.config.provisioner_key_b64)
            .map_err(|e| PiCloudError::ExternalCaError {
                reason: format!("invalid provisioner key base64: {e}"),
            })?;

        let signing_key =
            p256::ecdsa::SigningKey::from_slice(&key_bytes).map_err(|e| {
                PiCloudError::ExternalCaError {
                    reason: format!("invalid ES256 provisioner key: {e}"),
                }
            })?;

        use p256::ecdsa::signature::Signer;
        let signature: p256::ecdsa::Signature = signing_key.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

        Ok(format!("{signing_input}.{sig_b64}"))
    }

    /// Submit a signing request to step-ca's `/1.0/sign` endpoint.
    async fn sign_with_stepca(
        &self,
        csr_pem: &str,
        subject: &str,
        not_after: &str,
    ) -> Result<SignedCert> {
        let ott = self.create_provisioner_token(subject)?;

        let body = SignRequest {
            csr: csr_pem.to_string(),
            ott,
            not_after: not_after.to_string(),
        };

        let url = format!("{}/1.0/sign", self.config.endpoint);

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| PiCloudError::ExternalCaError {
                reason: format!("step-ca request failed: {e}"),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(PiCloudError::ExternalCaError {
                reason: format!("step-ca returned {status}: {text}"),
            });
        }

        let sign_resp: SignResponse =
            resp.json().await.map_err(|e| PiCloudError::ExternalCaError {
                reason: format!("failed to parse step-ca response: {e}"),
            })?;

        let fingerprint = compute_fingerprint(sign_resp.crt.as_bytes());

        // Parse the certificate to get the actual expiry
        let expires_at = parse_cert_expiry(&sign_resp.crt)?;

        Ok(SignedCert {
            cert_pem: sign_resp.crt,
            expires_at,
            fingerprint,
        })
    }

    /// Generate a CSR for signing by step-ca.
    ///
    /// Since the platform generates the keypair, the private key is returned
    /// alongside the PEM-encoded CSR.
    fn generate_csr(cn: &str, sans: &[String]) -> Result<(String, rcgen::KeyPair)> {
        let key =
            rcgen::KeyPair::generate().map_err(|e| PiCloudError::ExternalCaError {
                reason: format!("failed to generate key pair: {e}"),
            })?;

        let mut params = rcgen::CertificateParams::new(sans.to_vec()).map_err(|e| {
            PiCloudError::ExternalCaError {
                reason: format!("failed to create CSR params: {e}"),
            }
        })?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, cn);
        params
            .distinguished_name
            .push(rcgen::DnType::OrganizationName, "PiCloud");

        let csr = params.serialize_request(&key).map_err(|e| {
            PiCloudError::ExternalCaError {
                reason: format!("failed to serialize CSR: {e}"),
            }
        })?;

        Ok((csr.pem().map_err(|e| PiCloudError::ExternalCaError {
            reason: format!("failed to PEM-encode CSR: {e}"),
        })?, key))
    }
}

#[async_trait]
impl CertificateAuthority for SmallstepCa {
    async fn sign_node_csr(
        &self,
        _csr_der: &[u8],
        node_name: &str,
        node_ip: &str,
    ) -> Result<SignedCert> {
        let fqdn = format!("{node_name}.picloud.local");
        let sans = vec![fqdn.clone(), node_ip.to_string()];
        let (csr_pem, _key) = Self::generate_csr(&fqdn, &sans)?;

        let not_after = (Utc::now() + Duration::days(CertLifetime::NODE_DAYS))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        info!(node = %node_name, "Signing node certificate via step-ca");
        self.sign_with_stepca(&csr_pem, &fqdn, &not_after).await
    }

    async fn sign_workload_cert(
        &self,
        workload_iri: &ResourceIri,
        product: &str,
    ) -> Result<SignedCert> {
        let cn = format!("{product}.workload.picloud.local");
        let sans = vec![cn.clone()];
        let (csr_pem, _key) = Self::generate_csr(&cn, &sans)?;

        let not_after = (Utc::now() + Duration::hours(CertLifetime::WORKLOAD_HOURS))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        info!(workload = %workload_iri, "Signing workload certificate via step-ca");
        self.sign_with_stepca(&csr_pem, &cn, &not_after).await
    }

    async fn sign_ingress_cert(&self, hostname: &str) -> Result<SignedCert> {
        let sans = vec![hostname.to_string()];
        let (csr_pem, _key) = Self::generate_csr(hostname, &sans)?;

        let not_after = (Utc::now() + Duration::days(CertLifetime::INGRESS_DAYS))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        info!(hostname = %hostname, "Signing ingress certificate via step-ca");
        self.sign_with_stepca(&csr_pem, hostname, &not_after).await
    }

    fn ca_cert_pem(&self) -> &str {
        &self.config.ca_cert_pem
    }

    fn cert_lifetime(&self, cert_type: &CertType) -> StdDuration {
        CertLifetime::duration(cert_type)
    }
}

// ---------------------------------------------------------------------------
// Step-ca API types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SignRequest {
    csr: String,
    ott: String,
    #[serde(rename = "notAfter")]
    not_after: String,
}

#[derive(Deserialize)]
struct SignResponse {
    /// The signed certificate in PEM format.
    crt: String,
    /// The CA certificate chain in PEM format.
    #[allow(dead_code)]
    ca: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse the notAfter field from a PEM-encoded certificate.
fn parse_cert_expiry(cert_pem: &str) -> Result<chrono::DateTime<Utc>> {
    use x509_parser::pem::parse_x509_pem;

    let (_, pem) = parse_x509_pem(cert_pem.as_bytes()).map_err(|e| {
        PiCloudError::ExternalCaError {
            reason: format!("failed to parse certificate PEM: {e}"),
        }
    })?;

    let (_, cert) = x509_parser::parse_x509_certificate(&pem.contents).map_err(|e| {
        PiCloudError::ExternalCaError {
            reason: format!("failed to parse certificate DER: {e}"),
        }
    })?;

    let not_after = cert.validity().not_after.to_datetime();
    Ok(chrono::DateTime::<Utc>::from_naive_utc_and_offset(
        chrono::NaiveDateTime::new(
            chrono::NaiveDate::from_ymd_opt(
                not_after.year() as i32,
                not_after.month() as u32,
                not_after.day() as u32,
            )
            .unwrap_or_default(),
            chrono::NaiveTime::from_hms_opt(
                not_after.hour() as u32,
                not_after.minute() as u32,
                not_after.second() as u32,
            )
            .unwrap_or_default(),
        ),
        Utc,
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisioner_token_is_valid_jwt_structure() {
        let config = SmallstepConfig {
            endpoint: "https://step-ca.local:9000".to_string(),
            provisioner: "picloud".to_string(),
            // A valid 32-byte P-256 private key scalar, base64url-encoded.
            // This is a test-only key — not used in production.
            provisioner_key_b64: {
                let key = p256::ecdsa::SigningKey::random(&mut rand::thread_rng());
                URL_SAFE_NO_PAD.encode(key.to_bytes())
            },
            ca_cert_pem: "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----".to_string(),
            fingerprint: "sha256:abc123".to_string(),
        };

        let ca = SmallstepCa {
            config,
            client: reqwest::Client::new(),
        };

        let token = ca.create_provisioner_token("test-node.picloud.local").unwrap();

        // JWT has three base64url-encoded segments separated by dots
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 segments");

        // Decode and verify header
        let header_bytes = URL_SAFE_NO_PAD.decode(parts[0]).unwrap();
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(header["kid"], "picloud");

        // Decode and verify claims
        let claims_bytes = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&claims_bytes).unwrap();
        assert_eq!(claims["sub"], "test-node.picloud.local");
        assert_eq!(claims["iss"], "picloud");
        assert!(claims["exp"].is_number());
        assert!(claims["jti"].is_string());
    }

    #[test]
    fn generate_csr_produces_valid_pem() {
        let (csr_pem, _key) =
            SmallstepCa::generate_csr("test.picloud.local", &["test.picloud.local".to_string()])
                .unwrap();

        assert!(csr_pem.contains("-----BEGIN CERTIFICATE REQUEST-----"));
        assert!(csr_pem.contains("-----END CERTIFICATE REQUEST-----"));
    }
}
