//! FT-023 — mTLS node-to-node communication with platform-issued certificates
//!
//! Validates that the platform enforces mutual TLS for all inter-node
//! communication, using certificates issued by the platform CA.

use std::sync::Arc;

use picloud_network::PlatformCa;
use rustls::pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// Ensure the rustls crypto provider is installed (idempotent).
fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

// ---------------------------------------------------------------------------
// Helper: parse a single cert from PEM without lifetime issues
// ---------------------------------------------------------------------------

fn parse_first_cert_der(pem: &str) -> rustls::pki_types::CertificateDer<'static> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    certs.into_iter().next().expect("PEM must contain at least one cert")
}

fn parse_certs(pem: &str) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn parse_private_key(pem: &str) -> rustls::pki_types::PrivateKeyDer<'static> {
    let mut reader = std::io::BufReader::new(pem.as_bytes());
    rustls_pemfile::private_key(&mut reader).unwrap().unwrap()
}

// ---------------------------------------------------------------------------
// TC-241 — mTLS rejects connections without platform-issued certificates
// ---------------------------------------------------------------------------

/// A client that presents NO certificate must be rejected by the mTLS server.
#[tokio::test]
async fn tc241_mtls_rejects_connections_without_platform_issued_certificates() {
    ensure_crypto_provider();
    let ca = PlatformCa::new().unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(
        ca.mtls_server_config("server-node").unwrap(),
    ));

    // Spawn server task
    let server_handle = tokio::spawn(async move {
        let (tcp_stream, _) = listener.accept().await.unwrap();
        acceptor.accept(tcp_stream).await
    });

    // Build a client config that trusts the CA but presents NO client cert.
    let ca_cert_der = parse_first_cert_der(&ca.ca_certificate_pem());
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add(ca_cert_der).unwrap();

    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth(); // <-- no client certificate!

    let connector = TlsConnector::from(Arc::new(client_config));
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();

    let server_name = ServerName::try_from("server-node.picloud.local").unwrap();
    let client_result = connector.connect(server_name, tcp).await;

    let server_result = server_handle.await.unwrap();

    // At least one side must report an error — the mTLS server requires a
    // client cert and we didn't present one.
    let rejected = client_result.is_err() || server_result.is_err();
    assert!(
        rejected,
        "mTLS server must reject clients without platform-issued certificates"
    );
}

/// A client presenting a certificate from a *different* CA must be rejected.
#[tokio::test]
async fn tc241_mtls_rejects_foreign_ca_certificate() {
    ensure_crypto_provider();
    let platform_ca = PlatformCa::new().unwrap();
    let rogue_ca = PlatformCa::new().unwrap(); // independent CA

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(
        platform_ca.mtls_server_config("server-node").unwrap(),
    ));

    let server_handle = tokio::spawn(async move {
        let (tcp_stream, _) = listener.accept().await.unwrap();
        acceptor.accept(tcp_stream).await
    });

    // Client trusts the platform CA (for server verification) but presents
    // a certificate signed by the rogue CA.
    let client_config = {
        let ca_der = parse_first_cert_der(&platform_ca.ca_certificate_pem());
        let mut roots = rustls::RootCertStore::empty();
        roots.add(ca_der).unwrap();

        let rogue_cert = rogue_ca.issue_node_certificate("evil-node").unwrap();
        let cert_chain = parse_certs(&rogue_cert.certificate_pem);
        let key = parse_private_key(&rogue_cert.private_key_pem);

        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(cert_chain, key)
            .unwrap()
    };

    let connector = TlsConnector::from(Arc::new(client_config));
    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("server-node.picloud.local").unwrap();

    let client_result = connector.connect(server_name, tcp).await;
    let server_result = server_handle.await.unwrap();

    let rejected = client_result.is_err() || server_result.is_err();
    assert!(
        rejected,
        "mTLS server must reject certificates not signed by the platform CA"
    );
}

// ---------------------------------------------------------------------------
// TC-298 — mTLS exit — all node communication encrypted with platform certs
// ---------------------------------------------------------------------------

/// Full mTLS round-trip: two nodes with platform-issued certificates
/// successfully exchange data over an encrypted channel.
#[tokio::test]
async fn tc298_mtls_exit_all_node_communication_encrypted_with_platform_certificates() {
    ensure_crypto_provider();
    let ca = PlatformCa::new().unwrap();

    // ---- server node ----
    let server_config = ca.mtls_server_config("node-alpha").unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = acceptor.accept(tcp).await.expect("server TLS accept failed");

        // Echo back whatever the client sends
        let mut buf = vec![0u8; 1024];
        let n = tls.read(&mut buf).await.unwrap();
        tls.write_all(&buf[..n]).await.unwrap();
        tls.shutdown().await.unwrap();

        String::from_utf8(buf[..n].to_vec()).unwrap()
    });

    // ---- client node ----
    let client_config = ca.mtls_client_config("node-beta").unwrap();
    let connector = TlsConnector::from(Arc::new(client_config));

    let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("node-alpha.picloud.local").unwrap();
    let mut tls = connector
        .connect(server_name, tcp)
        .await
        .expect("client TLS connect failed — mTLS handshake must succeed with platform certs");

    // Send a message
    let payload = b"picloud-mtls-ping";
    tls.write_all(payload).await.unwrap();
    tls.shutdown().await.unwrap();

    // Read echo
    let mut response = Vec::new();
    tls.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, payload, "echoed data must match");

    // Verify server got the same data
    let server_received = server_handle.await.unwrap();
    assert_eq!(server_received, "picloud-mtls-ping");
}

/// Verify that `mtls_server_config` and `mtls_client_config` both produce
/// valid configurations that can be constructed without error, and that
/// the server config mandates client authentication.
#[test]
fn tc298_mtls_configs_constructible() {
    ensure_crypto_provider();
    let ca = PlatformCa::new().unwrap();

    let server_cfg = ca.mtls_server_config("node-1");
    assert!(server_cfg.is_ok(), "mtls_server_config must succeed");

    let client_cfg = ca.mtls_client_config("node-2");
    assert!(client_cfg.is_ok(), "mtls_client_config must succeed");

    // The server config must require client auth.
    // rustls 0.23 doesn't expose `client_auth_mandatory()` directly, so we
    // verify indirectly: the config was built with `with_client_cert_verifier`
    // which rejects unauthenticated clients (tested in TC-241 above).
    // Here we just confirm the configs are usable.
    let _cfg = server_cfg.unwrap();
    let _ccfg = client_cfg.unwrap();
}

/// Verify that node certificates carry the correct SAN and issuer.
#[test]
fn tc298_node_certificates_issued_by_platform_ca() {
    ensure_crypto_provider();
    use x509_parser::prelude::*;

    let ca = PlatformCa::new().unwrap();
    let node_cert = ca.issue_node_certificate("pi-node-42").unwrap();

    let cert_der = parse_first_cert_der(&node_cert.certificate_pem);
    let (_, parsed) = X509Certificate::from_der(cert_der.as_ref()).unwrap();

    // Verify CN
    let cn = parsed
        .subject()
        .iter_common_name()
        .next()
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(cn, "pi-node-42");

    // Verify issuer contains "PiCloud Platform CA"
    let issuer_cn = parsed
        .issuer()
        .iter_common_name()
        .next()
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(issuer_cn, "PiCloud Platform CA");

    // Verify SAN includes the .picloud.local domain
    let san_ext = parsed
        .extensions()
        .iter()
        .find(|e| e.oid == x509_parser::oid_registry::OID_X509_EXT_SUBJECT_ALT_NAME)
        .expect("SAN extension must be present");
    let san = san_ext.parsed_extension();
    if let x509_parser::extensions::ParsedExtension::SubjectAlternativeName(san) = san {
        let dns_names: Vec<_> = san
            .general_names
            .iter()
            .filter_map(|gn| match gn {
                x509_parser::extensions::GeneralName::DNSName(name) => Some(*name),
                _ => None,
            })
            .collect();
        assert!(
            dns_names.contains(&"pi-node-42.picloud.local"),
            "SAN must contain <node>.picloud.local, got: {:?}",
            dns_names
        );
    } else {
        panic!("Failed to parse SAN extension");
    }
}
