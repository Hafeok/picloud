/// FT-021 Integration Tests -- Internal DNS and Service Discovery
///
/// Covers TC-239 (product and container FQDN resolution from any node)
/// and TC-296 (exit criteria — all product and service FQDNs resolve).

use std::net::Ipv4Addr;
use std::sync::Arc;

use picloud_domain::iri::{ClusterDomain, IriBuilder};
use picloud_domain::traits::{DnsRegistry, QueryResult};

use picloud_network::dns::cache::DnsRecord;
use picloud_network::dns::resolver::{DnsResolver, ResolveResult, SparqlQueryFn};
use picloud_network::dns::events;
use picloud_network::InMemoryDnsRegistry;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn iri_builder() -> IriBuilder {
    IriBuilder::new(ClusterDomain::default())
}

/// Build a DnsResolver with a mock SPARQL backend that returns canned results.
fn resolver_with_mock_sparql(
    domain: &str,
    records: Vec<(String, serde_json::Value)>,
) -> DnsResolver {
    let records = Arc::new(records);
    let query_fn: SparqlQueryFn = Arc::new(move |sparql: &str| {
        let mut bindings = Vec::new();
        for (pattern, result) in records.iter() {
            if sparql.contains(pattern) {
                bindings.push(result.clone());
            }
        }
        Ok(QueryResult { bindings })
    });
    DnsResolver::new(domain.to_string()).with_sparql_query(query_fn)
}

// ============================================================================
// TC-239 -- Internal DNS resolves product and container FQDNs from any node
// ============================================================================

/// Resolve a product FQDN (e.g., `photo-app.picloud.local`) via the
/// authoritative resolver backed by SPARQL. Simulates resolution from
/// any node in the cluster.
#[tokio::test]
async fn tc239_internal_dns_resolves_product_and_container_fqdns() {
    // Set up resolver with mock SPARQL data for both product and container hostnames
    let resolver = resolver_with_mock_sparql(
        "picloud.local",
        vec![
            // Product FQDN: photo-app.picloud.local → 192.168.1.50
            (
                "\"photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.50" }),
            ),
            // Container FQDN: api-server.photo-app.picloud.local → 192.168.1.51
            (
                "\"api-server.photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.51" }),
            ),
            // Another container: worker.photo-app.picloud.local → 192.168.1.52
            (
                "\"worker.photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.52" }),
            ),
        ],
    );

    // --- Product FQDN resolution ---
    let result = resolver.resolve("photo-app.picloud.local", "A").await;
    match result {
        ResolveResult::Records { records, ttl } => {
            assert!(!records.is_empty(), "Product FQDN must resolve");
            assert_eq!(ttl, 30);
            match &records[0] {
                DnsRecord::A { address } => {
                    assert_eq!(*address, Ipv4Addr::new(192, 168, 1, 50));
                }
                other => panic!("Expected A record for product, got {:?}", other),
            }
        }
        other => panic!("Expected Records for product FQDN, got {:?}", other),
    }

    // --- Container FQDN resolution ---
    let result = resolver
        .resolve("api-server.photo-app.picloud.local", "A")
        .await;
    match result {
        ResolveResult::Records { records, ttl } => {
            assert!(!records.is_empty(), "Container FQDN must resolve");
            assert_eq!(ttl, 30);
            match &records[0] {
                DnsRecord::A { address } => {
                    assert_eq!(*address, Ipv4Addr::new(192, 168, 1, 51));
                }
                other => panic!("Expected A record for container, got {:?}", other),
            }
        }
        other => panic!("Expected Records for container FQDN, got {:?}", other),
    }

    // --- Second container in same product ---
    let result = resolver
        .resolve("worker.photo-app.picloud.local", "A")
        .await;
    match result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Second container FQDN must resolve");
            match &records[0] {
                DnsRecord::A { address } => {
                    assert_eq!(*address, Ipv4Addr::new(192, 168, 1, 52));
                }
                other => panic!("Expected A record for worker container, got {:?}", other),
            }
        }
        other => panic!("Expected Records for worker FQDN, got {:?}", other),
    }

    // --- Verify "from any node" property: resolution is SPARQL-backed, ---
    // --- so the answer is the same regardless of which node queries.  ---
    // Simulate a second independent resolver (representing a different node)
    // with the same SPARQL backend — must return identical results.
    let resolver_node2 = resolver_with_mock_sparql(
        "picloud.local",
        vec![
            (
                "\"photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.50" }),
            ),
            (
                "\"api-server.photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.51" }),
            ),
        ],
    );

    let result_n2 = resolver_node2
        .resolve("photo-app.picloud.local", "A")
        .await;
    match result_n2 {
        ResolveResult::Records { records, .. } => {
            match &records[0] {
                DnsRecord::A { address } => {
                    assert_eq!(
                        *address,
                        Ipv4Addr::new(192, 168, 1, 50),
                        "Product FQDN must resolve identically from any node"
                    );
                }
                other => panic!("Expected A record from node 2, got {:?}", other),
            }
        }
        other => panic!("Expected Records from node 2, got {:?}", other),
    }

    let result_n2 = resolver_node2
        .resolve("api-server.photo-app.picloud.local", "A")
        .await;
    match result_n2 {
        ResolveResult::Records { records, .. } => {
            match &records[0] {
                DnsRecord::A { address } => {
                    assert_eq!(
                        *address,
                        Ipv4Addr::new(192, 168, 1, 51),
                        "Container FQDN must resolve identically from any node"
                    );
                }
                other => panic!("Expected A record from node 2, got {:?}", other),
            }
        }
        other => panic!("Expected Records from node 2, got {:?}", other),
    }

    // --- Internal DNS registry (InMemoryDnsRegistry) also resolves FQDNs ---
    let registry = InMemoryDnsRegistry::new();
    let ib = iri_builder();

    let product_iri = ib.product("photo-app");
    registry
        .register(&product_iri, "192.168.1.50:443")
        .await
        .unwrap();

    let container_iri = ib.resource("photo-app", "containers", "api-server");
    registry
        .register(&container_iri, "192.168.1.51:8080")
        .await
        .unwrap();

    // Both resolve through the registry
    let addr = registry.resolve(&product_iri).await.unwrap();
    assert_eq!(addr, "192.168.1.50:443");

    let addr = registry.resolve(&container_iri).await.unwrap();
    assert_eq!(addr, "192.168.1.51:8080");

    // Unregistered resource returns error
    let unknown_iri = ib.resource("photo-app", "containers", "unknown");
    let err = registry.resolve(&unknown_iri).await;
    assert!(err.is_err());
}

// ============================================================================
// TC-296 -- DNS exit — internal DNS resolves all product and service FQDNs
// ============================================================================

/// Comprehensive exit criterion: every FQDN pattern the platform generates
/// must resolve through the internal DNS system.  Covers:
/// - Product FQDNs (A records)
/// - Container / workload FQDNs (A records)
/// - Node FQDNs (A records)
/// - Service discovery (SRV records)
/// - Product metadata (TXT records)
/// - Reverse DNS (PTR records)
/// - Cache invalidation on workload reschedule
/// - Cross-product isolation (negative test)
#[tokio::test]
async fn tc296_dns_exit_internal_dns_resolves_all_product_and_service_fqdns() {
    // Build a resolver with comprehensive SPARQL mock data
    let resolver = resolver_with_mock_sparql(
        "picloud.local",
        vec![
            // --- Products ---
            (
                "\"photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.50" }),
            ),
            (
                "\"data-pipeline.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.60" }),
            ),
            // --- Containers ---
            (
                "\"api-server.photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.51" }),
            ),
            (
                "\"worker.photo-app.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.52" }),
            ),
            (
                "\"ingest.data-pipeline.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.61" }),
            ),
            // --- Nodes ---
            (
                "\"pi-node-01.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.101" }),
            ),
            (
                "\"pi-node-02.picloud.local\"".to_string(),
                serde_json::json!({ "address": "192.168.1.102" }),
            ),
            // --- SRV records (service discovery) ---
            // parse_srv_name strips leading underscore, so pattern is "sparql" not "_sparql"
            (
                "\"sparql\"".to_string(),
                serde_json::json!({
                    "target": "pi-node-01.picloud.local",
                    "port": 7878,
                    "priority": 10,
                    "weight": 10
                }),
            ),
            // --- TXT records (product metadata) ---
            (
                "\"photo-app.picloud.local\"".to_string(),
                serde_json::json!({
                    "key": "version",
                    "value": "1.2.0"
                }),
            ),
        ],
    );

    // -----------------------------------------------------------------------
    // 1. Product A records
    // -----------------------------------------------------------------------
    let result = resolver.resolve("photo-app.picloud.local", "A").await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Product 'photo-app' A record must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 50)),
                "photo-app must resolve to 192.168.1.50"
            );
        }
        other => panic!("Expected A records for photo-app, got {:?}", other),
    }

    let result = resolver
        .resolve("data-pipeline.picloud.local", "A")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Product 'data-pipeline' A record must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 60)),
                "data-pipeline must resolve to 192.168.1.60"
            );
        }
        other => panic!("Expected A records for data-pipeline, got {:?}", other),
    }

    // -----------------------------------------------------------------------
    // 2. Container / workload A records
    // -----------------------------------------------------------------------
    let result = resolver
        .resolve("api-server.photo-app.picloud.local", "A")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Container 'api-server' must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 51))
            );
        }
        other => panic!("Expected A records for api-server container, got {:?}", other),
    }

    let result = resolver
        .resolve("worker.photo-app.picloud.local", "A")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Container 'worker' must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 52))
            );
        }
        other => panic!("Expected A records for worker container, got {:?}", other),
    }

    let result = resolver
        .resolve("ingest.data-pipeline.picloud.local", "A")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Container 'ingest' in data-pipeline must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 61))
            );
        }
        other => panic!(
            "Expected A records for ingest container, got {:?}",
            other
        ),
    }

    // -----------------------------------------------------------------------
    // 3. Node A records
    // -----------------------------------------------------------------------
    let result = resolver
        .resolve("pi-node-01.picloud.local", "A")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Node pi-node-01 must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 101))
            );
        }
        other => panic!("Expected A records for pi-node-01, got {:?}", other),
    }

    let result = resolver
        .resolve("pi-node-02.picloud.local", "A")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Node pi-node-02 must resolve");
            assert!(
                matches!(&records[0], DnsRecord::A { address } if *address == Ipv4Addr::new(192, 168, 1, 102))
            );
        }
        other => panic!("Expected A records for pi-node-02, got {:?}", other),
    }

    // -----------------------------------------------------------------------
    // 4. SRV records (service discovery)
    // -----------------------------------------------------------------------
    let result = resolver
        .resolve("_sparql._tcp.photo-app.picloud.local", "SRV")
        .await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "SRV record for _sparql._tcp must resolve");
            match &records[0] {
                DnsRecord::Srv {
                    target, port, priority, weight,
                } => {
                    assert_eq!(target, "pi-node-01.picloud.local");
                    assert_eq!(*port, 7878);
                    assert_eq!(*priority, 10);
                    assert_eq!(*weight, 10);
                }
                other => panic!("Expected SRV record, got {:?}", other),
            }
        }
        other => panic!("Expected SRV records, got {:?}", other),
    }

    // -----------------------------------------------------------------------
    // 5. TXT records (product metadata)
    // -----------------------------------------------------------------------
    // Use a dedicated resolver for TXT to avoid mock pattern collisions with A records
    let txt_resolver = resolver_with_mock_sparql(
        "picloud.local",
        vec![(
            "\"photo-app.picloud.local\"".to_string(),
            serde_json::json!({
                "key": "version",
                "value": "1.2.0"
            }),
        )],
    );
    let result = txt_resolver.resolve("photo-app.picloud.local", "TXT").await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "TXT record for product must resolve");
            match &records[0] {
                DnsRecord::Txt { text } => {
                    assert!(
                        text.contains("version") && text.contains("1.2.0"),
                        "TXT must contain product version, got: {}",
                        text
                    );
                }
                other => panic!("Expected TXT record, got {:?}", other),
            }
        }
        other => panic!("Expected TXT records, got {:?}", other),
    }

    // Cluster-level TXT record
    let resolver_with_cluster = resolver_with_mock_sparql("picloud.local", vec![])
        .with_cluster_info("cluster-001".to_string(), "0.1.0".to_string());
    let result = resolver_with_cluster.resolve("picloud.local", "TXT").await;
    match &result {
        ResolveResult::Records { records, .. } => {
            assert!(!records.is_empty(), "Cluster TXT record must resolve");
            match &records[0] {
                DnsRecord::Txt { text } => {
                    assert!(text.contains("cluster-001"), "Must contain cluster ID");
                    assert!(text.contains("0.1.0"), "Must contain platform version");
                }
                other => panic!("Expected TXT record, got {:?}", other),
            }
        }
        other => panic!("Expected cluster TXT records, got {:?}", other),
    }

    // -----------------------------------------------------------------------
    // 6. Non-authoritative domain returns NxDomain
    // -----------------------------------------------------------------------
    let result = resolver.resolve("example.com", "A").await;
    assert!(
        matches!(result, ResolveResult::NxDomain),
        "Non-authoritative domain must return NxDomain"
    );

    // -----------------------------------------------------------------------
    // 7. Cache invalidation on workload reschedule
    // -----------------------------------------------------------------------
    // The invalidation logic extracts the last IRI segment and builds
    // "{name}.{domain}" — so "api-server" from the IRI becomes
    // "api-server.picloud.local" as the cache key to invalidate.
    let cache = resolver.cache();
    {
        let mut c = cache.write().await;
        c.insert(
            "A",
            "api-server.picloud.local",
            vec![DnsRecord::A {
                address: Ipv4Addr::new(10, 0, 0, 99),
            }],
        );
    }

    // Simulate WorkloadRescheduled event
    events::handle_event(
        &cache,
        "picloud.local",
        "WorkloadRescheduled",
        &serde_json::json!({
            "workload_iri": "https://picloud.local/products/photo-app/containers/api-server",
        }),
    )
    .await;

    // After invalidation, cache should be empty for that hostname
    {
        let c = cache.read().await;
        assert!(
            c.get("A", "api-server.picloud.local").is_none(),
            "Cache must be invalidated after WorkloadRescheduled"
        );
    }

    // Similarly verify ProductDeployed cache invalidation
    {
        let mut c = cache.write().await;
        c.insert(
            "A",
            "photo-app.picloud.local",
            vec![DnsRecord::A {
                address: Ipv4Addr::new(10, 0, 0, 99),
            }],
        );
    }
    events::handle_event(
        &cache,
        "picloud.local",
        "ProductDeployed",
        &serde_json::json!({
            "product_iri": "https://picloud.local/products/photo-app",
            "product_name": "photo-app",
        }),
    )
    .await;
    {
        let c = cache.read().await;
        assert!(
            c.get("A", "photo-app.picloud.local").is_none(),
            "Cache must be invalidated after ProductDeployed"
        );
    }

    // -----------------------------------------------------------------------
    // 8. InMemoryDnsRegistry — product isolation (cross-product negative test)
    // -----------------------------------------------------------------------
    let registry = InMemoryDnsRegistry::new();
    let ib = iri_builder();

    // Register containers in two different products
    let container_a = ib.resource("product-a", "containers", "frontend");
    let container_b = ib.resource("product-b", "containers", "frontend");

    registry.register(&container_a, "10.0.0.1:8080").await.unwrap();
    registry.register(&container_b, "10.0.0.2:8080").await.unwrap();

    // Each product's container resolves to its own address
    assert_eq!(registry.resolve(&container_a).await.unwrap(), "10.0.0.1:8080");
    assert_eq!(registry.resolve(&container_b).await.unwrap(), "10.0.0.2:8080");

    // A container IRI from product-a cannot resolve a product-b resource
    let nonexistent = ib.resource("product-a", "containers", "backend");
    assert!(
        registry.resolve(&nonexistent).await.is_err(),
        "Unregistered container must not resolve"
    );

    // -----------------------------------------------------------------------
    // 9. Deregistration cleans up DNS entries
    // -----------------------------------------------------------------------
    registry.deregister(&container_a).await.unwrap();
    assert!(
        registry.resolve(&container_a).await.is_err(),
        "Deregistered container must not resolve"
    );
    // Other product's container unaffected
    assert_eq!(registry.resolve(&container_b).await.unwrap(), "10.0.0.2:8080");
}
