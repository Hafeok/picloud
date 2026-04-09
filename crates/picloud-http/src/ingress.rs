/// Ingress Event Handler (ADR-048)
///
/// Subscribes to the platform event stream and keeps the router
/// and TLS state in sync with ingress resource changes.
///
/// This is the bridge between the event log (source of truth)
/// and the in-memory router (operational state).
///
/// Events handled:
///   IngressCreated       -> upsert route, issue TLS cert
///   IngressUpdated       -> upsert route, refresh TLS cert if hostname changed
///   IngressDeleted       -> remove route, remove TLS cert
///   WorkloadRescheduled  -> update upstream address for all affected routes

use picloud_domain::error::Result;
use picloud_domain::iri::ResourceIri;

use crate::router::{RouteEntry, SharedRouter};
use crate::tls::SharedTls;

/// Handles ingress-related platform events and keeps the router in sync.
pub struct IngressEventHandler {
    router: SharedRouter,
    tls: SharedTls,
}

impl IngressEventHandler {
    pub fn new(router: SharedRouter, tls: SharedTls) -> Self {
        Self { router, tls }
    }

    /// Handle a platform event that affects ingress routing.
    /// Called from the event stream subscription loop.
    pub async fn handle(&self, event_type: &str, payload: &serde_json::Value) -> Result<()> {
        match event_type {
            "IngressCreated" | "IngressUpdated" => {
                self.on_ingress_upserted(payload).await?;
            }
            "IngressDeleted" => {
                self.on_ingress_deleted(payload).await?;
            }
            "WorkloadRescheduled" => {
                self.on_workload_rescheduled(payload).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn on_ingress_upserted(&self, payload: &serde_json::Value) -> Result<()> {
        let ingress_iri_str = payload["ingress_iri"].as_str().unwrap_or_default();
        let product = payload["product"].as_str().unwrap_or("unknown");
        let path_prefix = payload["path"].as_str().unwrap_or("/");
        let upstream_addr = payload["upstream_addr"]
            .as_str()
            .unwrap_or("127.0.0.1");
        let upstream_port = payload["port"].as_u64().unwrap_or(8080) as u16;
        let internal = payload["internal"].as_bool().unwrap_or(false);
        let hostname = payload["host"]
            .as_str()
            .unwrap_or("picloud.local")
            .to_string();

        let ingress_iri = ResourceIri::new(ingress_iri_str).unwrap_or_else(|_| {
            ResourceIri::new(format!(
                "https://picloud.local/products/{product}/ingresses/unknown"
            ))
            .unwrap()
        });

        // Issue TLS cert for external routes
        if !internal {
            self.tls.ensure_cert(&hostname).await?;
        }

        let entry = RouteEntry {
            path_prefix: path_prefix.to_string(),
            upstream_addr: upstream_addr.to_string(),
            upstream_port,
            product: product.to_string(),
            internal,
            ingress_iri,
        };

        self.router.upsert_route(hostname.clone(), entry).await;

        tracing::info!(
            hostname = %hostname,
            path = %path_prefix,
            upstream = %format!("{}:{}", upstream_addr, upstream_port),
            internal = internal,
            "Ingress route upserted"
        );
        Ok(())
    }

    async fn on_ingress_deleted(&self, payload: &serde_json::Value) -> Result<()> {
        let ingress_iri_str = payload["ingress_iri"].as_str().unwrap_or_default();
        let hostname = payload["host"].as_str().unwrap_or_default();

        if let Ok(iri) = ResourceIri::new(ingress_iri_str) {
            self.router.remove_route(&iri).await;
        }

        if !hostname.is_empty() {
            self.tls.remove_cert(hostname).await;
        }

        tracing::info!(
            ingress_iri = %ingress_iri_str,
            "Ingress route removed"
        );
        Ok(())
    }

    async fn on_workload_rescheduled(&self, payload: &serde_json::Value) -> Result<()> {
        let old_addr = payload["old_address"].as_str().unwrap_or_default();
        let new_addr = payload["new_address"].as_str().unwrap_or_default();

        if !old_addr.is_empty() && !new_addr.is_empty() {
            self.router.update_upstream(old_addr, new_addr).await;
        }

        tracing::info!(
            old_addr = %old_addr,
            new_addr = %new_addr,
            "Ingress upstream address updated after workload reschedule"
        );
        Ok(())
    }
}

/// Events that the ingress handler subscribes to.
/// Used to filter the platform event stream.
pub const SUBSCRIBED_EVENTS: &[&str] = &[
    "IngressCreated",
    "IngressUpdated",
    "IngressDeleted",
    "WorkloadRescheduled",
    "WorkloadFailed",
];
