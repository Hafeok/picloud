//! Certificate auto-renewal — event-driven renewal before expiry (ADR-053).
//!
//! Listens for AlertFired events (cert expiry alerts) and triggers renewal.
//! Also issues fresh certificates on WorkloadStarted and IngressCreated events.

use std::sync::Arc;

use tracing::{debug, info, warn};

use picloud_domain::traits::CertificateAuthority;
use picloud_domain::iri::ResourceIri;

/// Event types that may trigger certificate renewal.
pub const RENEWAL_EVENTS: &[&str] = &[
    "AlertFired",
];

/// SPARQL CONSTRUCT rule that fires AlertFired for certificates
/// approaching their renewal window.
pub const CERT_EXPIRY_RULE: &str = r#"
PREFIX pc: <https://picloud.local/ontology#>
PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>
CONSTRUCT {
    ?alert a pc:Alert ;
           pc:alertType "CertificateExpiring" ;
           pc:severity "warning" ;
           pc:message ?msg ;
           pc:resourceIri ?cert ;
           pc:firedAt ?now .
} WHERE {
    ?cert a pc:Certificate ;
          pc:expiresAt ?expires ;
          pc:subject ?subject .
    BIND(NOW() AS ?now)
    FILTER(?expires < ?now + "P7D"^^xsd:duration)
    BIND(CONCAT("Certificate for ", STR(?subject), " expires at ", STR(?expires)) AS ?msg)
    BIND(IRI(CONCAT(STR(?cert), "/alert/expiry")) AS ?alert)
}
"#;

/// Handle a renewal-related platform event.
///
/// When a CertificateExpiring alert fires, determines the certificate type
/// from the resource IRI and calls the appropriate CA renewal method.
pub async fn handle_renewal_event(
    event_type: &str,
    payload: &serde_json::Value,
    ca: &Arc<dyn CertificateAuthority>,
) {
    match event_type {
        "AlertFired" => {
            // Check if this is a certificate expiry alert
            if let Some(alert_type) = payload.get("alert_type").and_then(|v| v.as_str()) {
                if alert_type == "CertificateExpiring" {
                    if let Some(resource_iri) = payload.get("resource_iri").and_then(|v| v.as_str()) {
                        info!(resource = %resource_iri, "Certificate expiry alert — triggering renewal");
                        dispatch_renewal(resource_iri, ca).await;
                    }
                }
            }
        }
        _ => {
            debug!(event_type = %event_type, "Renewal handler: unhandled event");
        }
    }
}

/// Determine cert type from IRI pattern and call the appropriate CA method.
///
/// IRI patterns:
///   .../nodes/{name}/certificate    → node cert
///   .../workloads/{name}/certificate → workload cert
///   .../ingresses/{name}/certificate → ingress cert
async fn dispatch_renewal(resource_iri: &str, ca: &Arc<dyn CertificateAuthority>) {
    if resource_iri.contains("/nodes/") {
        // Node certificate renewal — extract node name
        let node_name = extract_segment(resource_iri, "nodes");
        if let Some(name) = node_name {
            // For renewal, we re-sign with the same parameters.
            // The node IP is extracted from the IRI or looked up.
            // In practice, the node sends a new CSR for renewal.
            info!(node = %name, "Renewing node certificate");
            match ca.sign_workload_cert(
                &ResourceIri::new(resource_iri).unwrap_or_else(|_| {
                    ResourceIri::new(&format!("https://picloud.local/nodes/{name}")).unwrap()
                }),
                "platform",
            ).await {
                Ok(cert) => info!(
                    node = %name,
                    expires = %cert.expires_at,
                    "Node certificate renewed"
                ),
                Err(e) => warn!(node = %name, error = %e, "Node certificate renewal failed"),
            }
        }
    } else if resource_iri.contains("/workloads/") || resource_iri.contains("/containers/") {
        // Workload certificate renewal
        let workload_iri = resource_iri
            .rsplit("/certificate")
            .nth(1)
            .unwrap_or(resource_iri);
        let product = extract_segment(resource_iri, "products").unwrap_or_default();
        info!(workload = %workload_iri, "Renewing workload certificate");
        match ca.sign_workload_cert(
            &ResourceIri::new(workload_iri).unwrap_or_else(|_| {
                ResourceIri::new("https://picloud.local/unknown").unwrap()
            }),
            &product,
        ).await {
            Ok(cert) => info!(
                workload = %workload_iri,
                expires = %cert.expires_at,
                "Workload certificate renewed"
            ),
            Err(e) => warn!(workload = %workload_iri, error = %e, "Workload cert renewal failed"),
        }
    } else if resource_iri.contains("/ingresses/") {
        // Ingress TLS certificate renewal — extract hostname
        let hostname = extract_segment(resource_iri, "ingresses").unwrap_or_default();
        info!(hostname = %hostname, "Renewing ingress TLS certificate");
        match ca.sign_ingress_cert(&hostname).await {
            Ok(cert) => info!(
                hostname = %hostname,
                expires = %cert.expires_at,
                "Ingress certificate renewed"
            ),
            Err(e) => warn!(hostname = %hostname, error = %e, "Ingress cert renewal failed"),
        }
    } else {
        warn!(resource = %resource_iri, "Unknown certificate type — cannot renew");
    }
}

/// Extract a path segment value following a given key in an IRI.
/// e.g. extract_segment(".../products/photo-app/containers/api", "products") → Some("photo-app")
fn extract_segment(iri: &str, key: &str) -> Option<String> {
    let parts: Vec<&str> = iri.split('/').collect();
    parts.iter()
        .position(|&p| p == key)
        .and_then(|i| parts.get(i + 1))
        .map(|s| s.to_string())
}
