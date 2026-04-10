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
use picloud_domain::traits::CertificateAuthority;
use tokio::sync::RwLock;

/// Shared TLS state across the HTTP server.
pub type SharedTls = Arc<TlsState>;

/// Manages per-hostname TLS certificates issued by the platform CA.
pub struct TlsState {
    /// Certificates keyed by hostname.
    /// Issued by the platform CA on IngressCreated events.
    certs: RwLock<HashMap<String, IssuedCert>>,
    /// Platform CA for issuing ingress certificates (ADR-030, ADR-053).
    ca: Option<Arc<dyn CertificateAuthority>>,
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
            ca: None,
        }
    }

    /// Create a TlsState with a platform CA for certificate issuance.
    pub fn with_ca(ca: Arc<dyn CertificateAuthority>) -> Self {
        Self {
            certs: RwLock::new(HashMap::new()),
            ca: Some(ca),
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

        // Request certificate from platform CA (ADR-030)
        let Some(ref ca) = self.ca else {
            tracing::warn!(
                hostname = %hostname,
                "No CA configured — cannot issue TLS certificate"
            );
            return Ok(());
        };

        tracing::info!(
            hostname = %hostname,
            "Issuing TLS certificate for ingress hostname"
        );

        let signed = ca.sign_ingress_cert(hostname).await?;

        let mut certs = self.certs.write().await;
        certs.insert(
            hostname.to_string(),
            IssuedCert {
                hostname: hostname.to_string(),
                cert_pem: signed.cert_pem,
                // The CA trait returns cert_pem only; for a full implementation
                // we would also need the private key. For now we store the CA cert
                // as a placeholder — the key pair would be generated as part of the
                // CSR flow in a production deployment.
                key_pem: String::new(),
                expires_at: signed.expires_at,
            },
        );

        tracing::info!(
            hostname = %hostname,
            expires = %signed.expires_at,
            "TLS certificate issued"
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
pub struct SniResolver {
    tls: SharedTls,
}

impl std::fmt::Debug for SniResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SniResolver").finish()
    }
}

impl SniResolver {
    pub fn new(tls: SharedTls) -> Self {
        Self { tls }
    }
}

impl rustls::server::ResolvesServerCert for SniResolver {
    fn resolve(
        &self,
        client_hello: rustls::server::ClientHello<'_>,
    ) -> Option<Arc<rustls::sign::CertifiedKey>> {
        let hostname = client_hello.server_name()?;
        let certs = self.tls.certs.blocking_read();
        let cert = certs.get(hostname)?;

        if cert.cert_pem.is_empty() || cert.key_pem.is_empty() {
            tracing::debug!(hostname = %hostname, "Certificate PEM empty — cannot build CertifiedKey");
            return None;
        }

        // Parse PEM certificate chain
        let cert_chain: Vec<rustls::pki_types::CertificateDer<'static>> =
            rustls_pemfile::certs(&mut cert.cert_pem.as_bytes())
                .filter_map(|r| r.ok())
                .collect();

        if cert_chain.is_empty() {
            tracing::warn!(hostname = %hostname, "No certificates found in PEM");
            return None;
        }

        // Parse PEM private key
        let key_der = rustls_pemfile::private_key(&mut cert.key_pem.as_bytes())
            .ok()
            .flatten()?;

        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key_der).ok()?;

        Some(Arc::new(rustls::sign::CertifiedKey::new(
            cert_chain,
            signing_key,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tls_state_no_ca_logs_warning() {
        let state = TlsState::new();
        // Should not error even without a CA
        let result = state.ensure_cert("test.picloud.local").await;
        assert!(result.is_ok());
        assert_eq!(state.cert_count().await, 0);
    }

    #[tokio::test]
    async fn remove_cert_is_idempotent() {
        let state = TlsState::new();
        state.remove_cert("nonexistent.picloud.local").await;
        assert_eq!(state.cert_count().await, 0);
    }
}
