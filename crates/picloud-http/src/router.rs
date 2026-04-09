/// Ingress Router (ADR-048)
///
/// Maintains an in-memory routing table rebuilt from the RDF graph
/// on every IngressCreated, IngressUpdated, IngressDeleted, and
/// WorkloadRescheduled event.
///
/// Lookups are O(1) by hostname, O(n) path prefix match within a host
/// (n = number of routes per host, typically very small).
///
/// No config files. No process restarts. The routing table IS the RDF graph.

use std::collections::HashMap;
use std::sync::Arc;

use picloud_domain::iri::ResourceIri;
use tokio::sync::RwLock;

/// The complete router state — shared across the HTTP server threads.
pub type SharedRouter = Arc<IngressRouter>;

/// The ingress router holds all external and internal routes.
/// All mutation goes through `&self` methods that acquire write locks internally.
pub struct IngressRouter {
    /// External routes — TLS terminated, publicly reachable on port 443.
    /// Keyed by hostname (e.g. "photos.picloud.local").
    external: RwLock<HashMap<String, Vec<RouteEntry>>>,
    /// Internal routes — mTLS mesh only, never externally reachable.
    /// Used for metrics, health checks, debug ports.
    internal: RwLock<Vec<RouteEntry>>,
}

#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// Path prefix to match (e.g. "/api", "/"). Longest prefix wins.
    pub path_prefix: String,
    /// Upstream address and port for the workload container.
    pub upstream_addr: String,
    pub upstream_port: u16,
    /// Product that owns this route.
    pub product: String,
    /// Whether this is an internal-only route.
    pub internal: bool,
    /// IRI of the ingress resource that created this route.
    pub ingress_iri: ResourceIri,
}

impl IngressRouter {
    pub fn new() -> Self {
        Self {
            external: RwLock::new(HashMap::new()),
            internal: RwLock::new(Vec::new()),
        }
    }

    /// Look up a route for an incoming request by hostname and path.
    /// Returns the best matching route (longest path prefix wins).
    pub async fn lookup(&self, host: &str, path: &str) -> Option<RouteEntry> {
        let external = self.external.read().await;
        if let Some(routes) = external.get(host) {
            let best = routes
                .iter()
                .filter(|r| path.starts_with(&r.path_prefix))
                .max_by_key(|r| r.path_prefix.len());
            if let Some(entry) = best {
                return Some(entry.clone());
            }
        }
        None
    }

    /// Look up a route in the internal table by path.
    pub async fn lookup_internal(&self, path: &str) -> Option<RouteEntry> {
        let internal = self.internal.read().await;
        internal
            .iter()
            .filter(|r| path.starts_with(&r.path_prefix))
            .max_by_key(|r| r.path_prefix.len())
            .cloned()
    }

    /// Add or update a route — called on IngressCreated / IngressUpdated events.
    pub async fn upsert_route(&self, hostname: String, entry: RouteEntry) {
        if entry.internal {
            let mut internal = self.internal.write().await;
            internal.retain(|r| r.ingress_iri != entry.ingress_iri);
            internal.push(entry);
        } else {
            let mut external = self.external.write().await;
            let routes = external.entry(hostname).or_default();
            routes.retain(|r| r.ingress_iri != entry.ingress_iri);
            routes.push(entry);
        }
    }

    /// Remove a route by ingress IRI — called on IngressDeleted events.
    pub async fn remove_route(&self, ingress_iri: &ResourceIri) {
        {
            let mut external = self.external.write().await;
            for routes in external.values_mut() {
                routes.retain(|r| &r.ingress_iri != ingress_iri);
            }
            // Remove empty hostname entries
            external.retain(|_, routes| !routes.is_empty());
        }
        {
            let mut internal = self.internal.write().await;
            internal.retain(|r| &r.ingress_iri != ingress_iri);
        }
    }

    /// Update the upstream address when a workload reschedules to a new node.
    pub async fn update_upstream(&self, old_addr: &str, new_addr: &str) {
        {
            let mut external = self.external.write().await;
            for routes in external.values_mut() {
                for route in routes.iter_mut() {
                    if route.upstream_addr == old_addr {
                        route.upstream_addr = new_addr.to_string();
                    }
                }
            }
        }
        {
            let mut internal = self.internal.write().await;
            for route in internal.iter_mut() {
                if route.upstream_addr == old_addr {
                    route.upstream_addr = new_addr.to_string();
                }
            }
        }
    }

    /// Total number of routes across all tables.
    pub async fn route_count(&self) -> usize {
        let ext: usize = self.external.read().await.values().map(|v| v.len()).sum();
        let int = self.internal.read().await.len();
        ext + int
    }
}

impl Default for IngressRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(path: &str, addr: &str, port: u16, product: &str, internal: bool) -> RouteEntry {
        RouteEntry {
            path_prefix: path.to_string(),
            upstream_addr: addr.to_string(),
            upstream_port: port,
            product: product.to_string(),
            internal,
            ingress_iri: ResourceIri::new(format!(
                "https://picloud.local/products/{product}/ingresses/test-{port}"
            ))
            .unwrap(),
        }
    }

    #[tokio::test]
    async fn test_lookup_by_host_and_path() {
        let router = IngressRouter::new();

        let entry = make_entry("/api", "10.0.0.1", 8080, "photo-app", false);
        router
            .upsert_route("photos.picloud.local".to_string(), entry)
            .await;

        let root_entry = make_entry("/", "10.0.0.1", 3000, "photo-app", false);
        router
            .upsert_route("photos.picloud.local".to_string(), root_entry)
            .await;

        // Should match /api (longest prefix)
        let result = router
            .lookup("photos.picloud.local", "/api/v1/photos")
            .await;
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.path_prefix, "/api");
        assert_eq!(r.upstream_port, 8080);

        // Should match / (root)
        let result = router.lookup("photos.picloud.local", "/index.html").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().upstream_port, 3000);

        // No match for unknown host
        let result = router.lookup("unknown.picloud.local", "/api").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_upsert_replaces_existing_route() {
        let router = IngressRouter::new();

        let entry1 = make_entry("/api", "10.0.0.1", 8080, "photo-app", false);
        router
            .upsert_route("photos.picloud.local".to_string(), entry1)
            .await;

        // Upsert with same ingress_iri but different upstream
        let mut entry2 = make_entry("/api", "10.0.0.2", 9090, "photo-app", false);
        entry2.ingress_iri = ResourceIri::new(
            "https://picloud.local/products/photo-app/ingresses/test-8080",
        )
        .unwrap();
        router
            .upsert_route("photos.picloud.local".to_string(), entry2)
            .await;

        assert_eq!(router.route_count().await, 1);
        let result = router.lookup("photos.picloud.local", "/api/test").await;
        assert_eq!(result.unwrap().upstream_port, 9090);
    }

    #[tokio::test]
    async fn test_remove_route() {
        let router = IngressRouter::new();

        let entry = make_entry("/api", "10.0.0.1", 8080, "photo-app", false);
        let iri = entry.ingress_iri.clone();
        router
            .upsert_route("photos.picloud.local".to_string(), entry)
            .await;

        assert_eq!(router.route_count().await, 1);
        router.remove_route(&iri).await;
        assert_eq!(router.route_count().await, 0);

        let result = router.lookup("photos.picloud.local", "/api").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_internal_routes() {
        let router = IngressRouter::new();

        let entry = make_entry("/metrics", "10.0.0.1", 9090, "photo-app", true);
        router
            .upsert_route("".to_string(), entry)
            .await;

        // Internal routes are not found via external lookup
        let result = router.lookup("photos.picloud.local", "/metrics").await;
        assert!(result.is_none());

        // But found via internal lookup
        let result = router.lookup_internal("/metrics").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().upstream_port, 9090);
    }
}
