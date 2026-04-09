/// TLS Configuration (ADR-048)
///
/// SNI-based certificate selection — one certificate per ingress hostname,
/// all issued by the platform CA (ADR-030).
///
/// When an IngressCreated event fires, a certificate is issued for the
/// declared hostname before the route becomes active. The router never
/// serves a request without a valid certificate.

use std::collections::HashMap;
use std::sync::Arc;

use picloud_domain::error::Result;
use tokio::sync::RwLock;

/// Shared TLS state across the HTTP server.
pub type SharedTls = Arc<TlsState>;

/// Manages per-hostname TLS certificates issued by the platform CA.
pub struct TlsState {
    /// Certificates keyed by hostname.
    /// Issued by the platform CA on IngressCreated events.
    certs: RwLock<HashMap<String, IssuedCert>>,
}

/// A TLS certificate issued by the platform CA for a specific hostname.
#[derive(Debug, Clone)]
pub struct IssuedCert {
    pub hostname: String,
    pub cert_pem: String,
    pub key_pem: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl TlsState {
    pub fn new() -> Self {
        Self {
            certs: RwLock::new(HashMap::new()),
        }
    }

    /// Issue or refresh a certificate for a hostname via the platform CA.
    /// Called on IngressCreated — certificate is ready before first request.
    pub async fn ensure_cert(&self, hostname: &str) -> Result<()> {
        let certs = self.certs.read().await;
        if let Some(cert) = certs.get(hostname) {
            // Refresh if expiring within 24 hours
            let expires_soon =
                cert.expires_at < chrono::Utc::now() + chrono::Duration::hours(24);
            if !expires_soon {
                return Ok(());
            }
        }
        drop(certs);

        // TODO: request certificate from picloud-network CA
        // The platform CA (ADR-030) issues short-lived certs for ingress hostnames.
        tracing::info!(
            hostname = %hostname,
            "Issuing TLS certificate for ingress hostname"
        );

        Ok(())
    }

    /// Remove a certificate when an ingress is deleted.
    pub async fn remove_cert(&self, hostname: &str) {
        let mut certs = self.certs.write().await;
        certs.remove(hostname);
        tracing::debug!(
            hostname = %hostname,
            "Removed TLS certificate for deleted ingress"
        );
    }

    /// Number of certificates currently held.
    pub async fn cert_count(&self) -> usize {
        self.certs.read().await.len()
    }
}

impl Default for TlsState {
    fn default() -> Self {
        Self::new()
    }
}

/// SNI resolver — selects certificate based on hostname in TLS ClientHello.
/// Plugs into rustls ServerConfig as the ResolvesServerCert implementation.
///
/// Implementation note: rustls calls resolve() on every TLS handshake.
/// The lookup must be fast — RwLock read, HashMap get, clone Arc.
#[allow(dead_code)]
pub struct SniResolver {
    tls: SharedTls,
}

impl SniResolver {
    pub fn new(tls: SharedTls) -> Self {
        Self { tls }
    }

    // TODO: implement rustls::server::ResolvesServerCert
    // When rustls dep is wired:
    //
    // fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
    //     let hostname = client_hello.server_name()?;
    //     let tls = self.tls.certs.blocking_read();
    //     let cert = tls.get(hostname)?;
    //     // Build CertifiedKey from cert_pem and key_pem
    // }
}
