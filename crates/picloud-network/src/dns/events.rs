//! DNS cache invalidation via platform events.
//!
//! Subscribes to events that indicate DNS-relevant state has changed
//! and invalidates the appropriate cache entries.

use tracing::debug;

use super::cache::SharedDnsCache;

/// Event types that trigger DNS cache invalidation.
pub const DNS_INVALIDATION_EVENTS: &[&str] = &[
    "WorkloadRescheduled",
    "IngressCreated",
    "IngressUpdated",
    "IngressDeleted",
    "NodeJoined",
    "NodeLeft",
    "ProductDeployed",
    "ProductUpgradeCompleted",
];

/// Handle a platform event and invalidate DNS cache if relevant.
pub async fn handle_event(
    cache: &SharedDnsCache,
    domain: &str,
    event_type: &str,
    payload: &serde_json::Value,
) {
    match event_type {
        "WorkloadRescheduled" => invalidate_workload_hostname(cache, domain, payload).await,
        "IngressCreated" | "IngressUpdated" => invalidate_ingress(cache, payload).await,
        "IngressDeleted" => invalidate_ingress(cache, payload).await,
        "NodeJoined" | "NodeLeft" => invalidate_node(cache, domain, payload).await,
        "ProductDeployed" | "ProductUpgradeCompleted" => invalidate_product(cache, domain, payload).await,
        _ => {
            debug!(event_type = %event_type, "DNS event handler: unhandled event type");
        }
    }
}

async fn invalidate_workload_hostname(
    cache: &SharedDnsCache,
    domain: &str,
    payload: &serde_json::Value,
) {
    // Invalidate any cached records for the rescheduled workload's hostname
    if let Some(workload_iri) = payload.get("workload_iri").and_then(|v| v.as_str()) {
        // Extract the workload name from the IRI to build the hostname
        if let Some(name) = workload_iri.rsplit('/').next() {
            let hostname = format!("{name}.{domain}");
            let mut cache = cache.write().await;
            cache.invalidate(&hostname);
        }
    }
}

async fn invalidate_ingress(cache: &SharedDnsCache, payload: &serde_json::Value) {
    // Invalidate cached records for the ingress hostname
    if let Some(hostname) = payload.get("hostname").and_then(|v| v.as_str()) {
        let mut cache = cache.write().await;
        cache.invalidate(hostname);
    }
}

async fn invalidate_node(cache: &SharedDnsCache, domain: &str, payload: &serde_json::Value) {
    if let Some(node_name) = payload.get("node_name").and_then(|v| v.as_str()) {
        let hostname = format!("{node_name}.{domain}");
        let mut cache = cache.write().await;
        cache.invalidate(&hostname);
    }
}

async fn invalidate_product(cache: &SharedDnsCache, domain: &str, payload: &serde_json::Value) {
    if let Some(product_iri) = payload.get("product_iri").and_then(|v| v.as_str()) {
        if let Some(name) = product_iri.rsplit('/').next() {
            let hostname = format!("{name}.{domain}");
            let mut cache = cache.write().await;
            cache.invalidate(&hostname);
        }
    }
}
