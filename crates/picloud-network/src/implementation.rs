//! Network implementation: in-memory DNS registry and platform CA.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use picloud_domain::error::{PiCloudError, Result};
use picloud_domain::iri::ResourceIri;
use picloud_domain::traits::DnsRegistry;

// ---------------------------------------------------------------------------
// In-memory DNS Registry
// ---------------------------------------------------------------------------

/// An in-memory DNS registry mapping resource IRIs to network addresses.
pub struct InMemoryDnsRegistry {
    records: RwLock<HashMap<String, String>>,
}

impl InMemoryDnsRegistry {
    /// Create a new, empty DNS registry.
    pub fn new() -> Self {
        Self {
            records: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryDnsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DnsRegistry for InMemoryDnsRegistry {
    async fn register(&self, iri: &ResourceIri, address: &str) -> Result<()> {
        let mut records = self.records.write().await;
        info!(iri = %iri, address = %address, "DNS record registered");
        records.insert(iri.as_str().to_string(), address.to_string());
        Ok(())
    }

    async fn deregister(&self, iri: &ResourceIri) -> Result<()> {
        let mut records = self.records.write().await;
        if records.remove(iri.as_str()).is_some() {
            info!(iri = %iri, "DNS record deregistered");
            Ok(())
        } else {
            warn!(iri = %iri, "DNS deregister: record not found");
            Err(PiCloudError::ResourceNotFound {
                iri: iri.as_str().to_string(),
            })
        }
    }

    async fn resolve(&self, iri: &ResourceIri) -> Result<String> {
        let records = self.records.read().await;
        match records.get(iri.as_str()) {
            Some(address) => {
                debug!(iri = %iri, address = %address, "DNS resolved");
                Ok(address.clone())
            }
            None => {
                warn!(iri = %iri, "DNS resolve: record not found");
                Err(PiCloudError::ResourceNotFound {
                    iri: iri.as_str().to_string(),
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TLS Certificate Management
// ---------------------------------------------------------------------------

/// A certificate issued for a cluster node.
#[derive(Debug, Clone)]
pub struct NodeCertificate {
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub expires_at: DateTime<Utc>,
}

/// A certificate issued for a named service.
#[derive(Debug, Clone)]
pub struct ServiceCertificate {
    pub certificate_pem: String,
    pub private_key_pem: String,
    pub expires_at: DateTime<Utc>,
}

/// Platform Certificate Authority — generates a self-signed root CA and
/// issues node / service certificates signed by that CA.
pub struct PlatformCa {
    ca_cert_pem: String,
    ca_params: CertificateParams,
    ca_key: KeyPair,
}

impl PlatformCa {
    /// Generate a new self-signed root CA.
    pub fn new() -> Result<Self> {
        let ca_key =
            KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to generate CA key pair: {e}"),
            })?;

        let mut ca_params =
            CertificateParams::new(Vec::<String>::new()).map_err(|e| {
                PiCloudError::TlsCertificateError {
                    reason: format!("failed to create CA params: {e}"),
                }
            })?;

        ca_params
            .distinguished_name
            .push(DnType::CommonName, "PiCloud Platform CA");
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

        let ca_cert_pem = ca_cert.pem();

        info!("Platform CA initialized");

        Ok(Self {
            ca_cert_pem,
            ca_params,
            ca_key,
        })
    }

    /// Export the CA certificate as PEM.
    pub fn ca_certificate_pem(&self) -> String {
        self.ca_cert_pem.clone()
    }

    /// Issue a TLS certificate for a cluster node, signed by the platform CA.
    pub fn issue_node_certificate(&self, node_name: &str) -> Result<NodeCertificate> {
        let expires_at = Utc::now() + Duration::days(365);

        let ee_key =
            KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to generate node key pair: {e}"),
            })?;

        let mut params = CertificateParams::new(vec![
            node_name.to_string(),
            format!("{node_name}.picloud.local"),
        ])
        .map_err(|e| PiCloudError::TlsCertificateError {
            reason: format!("failed to create node cert params: {e}"),
        })?;

        params
            .distinguished_name
            .push(DnType::CommonName, node_name);
        params
            .distinguished_name
            .push(DnType::OrganizationName, "PiCloud");

        let ca_cert = self.ca_params.clone().self_signed(&self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to reconstruct CA cert for signing: {e}"),
            }
        })?;

        let cert = params
            .signed_by(&ee_key, &ca_cert, &self.ca_key)
            .map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to sign node certificate: {e}"),
            })?;

        info!(node = %node_name, "Node certificate issued");

        Ok(NodeCertificate {
            certificate_pem: cert.pem(),
            private_key_pem: ee_key.serialize_pem(),
            expires_at,
        })
    }

    /// Issue a TLS certificate for a service, signed by the platform CA.
    pub fn issue_service_certificate(
        &self,
        service_name: &str,
        domain: &str,
    ) -> Result<ServiceCertificate> {
        let expires_at = Utc::now() + Duration::days(90);

        let ee_key =
            KeyPair::generate().map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to generate service key pair: {e}"),
            })?;

        let fqdn = format!("{service_name}.{domain}");
        let mut params = CertificateParams::new(vec![fqdn.clone()]).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to create service cert params: {e}"),
            }
        })?;

        params
            .distinguished_name
            .push(DnType::CommonName, fqdn.as_str());
        params
            .distinguished_name
            .push(DnType::OrganizationName, "PiCloud");

        let ca_cert = self.ca_params.clone().self_signed(&self.ca_key).map_err(|e| {
            PiCloudError::TlsCertificateError {
                reason: format!("failed to reconstruct CA cert for signing: {e}"),
            }
        })?;

        let cert = params
            .signed_by(&ee_key, &ca_cert, &self.ca_key)
            .map_err(|e| PiCloudError::TlsCertificateError {
                reason: format!("failed to sign service certificate: {e}"),
            })?;

        info!(service = %service_name, domain = %domain, "Service certificate issued");

        Ok(ServiceCertificate {
            certificate_pem: cert.pem(),
            private_key_pem: ee_key.serialize_pem(),
            expires_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_iri(path: &str) -> ResourceIri {
        ResourceIri::new(format!("https://picloud.local/{path}")).unwrap()
    }

    #[tokio::test]
    async fn dns_register_and_resolve_round_trip() {
        let registry = InMemoryDnsRegistry::new();
        let iri = test_iri("nodes/pi-01");

        registry.register(&iri, "192.168.1.10:8443").await.unwrap();
        let addr = registry.resolve(&iri).await.unwrap();

        assert_eq!(addr, "192.168.1.10:8443");
    }

    #[tokio::test]
    async fn dns_deregister_then_resolve_returns_error() {
        let registry = InMemoryDnsRegistry::new();
        let iri = test_iri("nodes/pi-02");

        registry.register(&iri, "192.168.1.20:8443").await.unwrap();
        registry.deregister(&iri).await.unwrap();

        let result = registry.resolve(&iri).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::ResourceNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn dns_deregister_missing_returns_error() {
        let registry = InMemoryDnsRegistry::new();
        let iri = test_iri("nodes/nonexistent");

        let result = registry.deregister(&iri).await;
        assert!(matches!(
            result.unwrap_err(),
            PiCloudError::ResourceNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn dns_multiple_records() {
        let registry = InMemoryDnsRegistry::new();
        let iri1 = test_iri("nodes/pi-01");
        let iri2 = test_iri("nodes/pi-02");
        let iri3 = test_iri("products/app/containers/web");

        registry.register(&iri1, "10.0.0.1:8443").await.unwrap();
        registry.register(&iri2, "10.0.0.2:8443").await.unwrap();
        registry.register(&iri3, "10.0.0.3:9090").await.unwrap();

        assert_eq!(registry.resolve(&iri1).await.unwrap(), "10.0.0.1:8443");
        assert_eq!(registry.resolve(&iri2).await.unwrap(), "10.0.0.2:8443");
        assert_eq!(registry.resolve(&iri3).await.unwrap(), "10.0.0.3:9090");
    }

    #[test]
    fn ca_generates_valid_pem() {
        let ca = PlatformCa::new().unwrap();
        let pem = ca.ca_certificate_pem();

        assert!(pem.contains("-----BEGIN CERTIFICATE-----"));
        assert!(pem.contains("-----END CERTIFICATE-----"));
    }

    #[test]
    fn ca_issues_node_certificate() {
        let ca = PlatformCa::new().unwrap();
        let node_cert = ca.issue_node_certificate("pi-node-01").unwrap();

        assert!(node_cert
            .certificate_pem
            .contains("-----BEGIN CERTIFICATE-----"));
        assert!(node_cert
            .private_key_pem
            .contains("-----BEGIN PRIVATE KEY-----"));
        assert!(node_cert.expires_at > Utc::now());
    }

    #[test]
    fn ca_issues_service_certificate() {
        let ca = PlatformCa::new().unwrap();
        let svc_cert = ca
            .issue_service_certificate("api-server", "picloud.local")
            .unwrap();

        assert!(svc_cert
            .certificate_pem
            .contains("-----BEGIN CERTIFICATE-----"));
        assert!(svc_cert
            .private_key_pem
            .contains("-----BEGIN PRIVATE KEY-----"));
        assert!(svc_cert.expires_at > Utc::now());
    }
}
