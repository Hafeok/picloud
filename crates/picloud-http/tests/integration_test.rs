/// Integration test for the full resource apply pipeline.
///
/// Tests the end-to-end flow: POST /api/apply → events emitted → RDF projection → SPARQL query.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use picloud_domain::iri::ClusterDomain;
use picloud_domain::traits::{EventFilter, EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use picloud_rdf::OxigraphProjector;

/// Build a test server with real event log and projector wired together.
fn test_server_with_deps() -> (axum::Router, Arc<InMemoryEventLog>, Arc<OxigraphProjector>) {
    let domain = ClusterDomain::default();

    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let server = PiCloudHttpServer::new("127.0.0.1:0".parse().unwrap(), domain).with_dependencies(
        event_log.clone(),
        projector.clone(),
        // No cluster, iam, storage, scheduler for this test
        Arc::new(FakeCluster),
        Arc::new(FakeIam),
        Arc::new(FakeStorage),
        Arc::new(FakeScheduler),
    );

    (server.build_router(), event_log, projector)
}

/// Build a test server with projection loop — subscribe before applying.
async fn test_server_with_projection() -> (
    axum::Router,
    Arc<InMemoryEventLog>,
    Arc<OxigraphProjector>,
) {
    let (router, event_log, projector) = test_server_with_deps();

    // Subscribe BEFORE any events are sent, so we catch them all
    let proj = projector.clone();
    let mut rx = event_log.subscribe(EventFilter::default()).await.unwrap();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let _ = proj.project(&event).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    (router, event_log, projector)
}

#[tokio::test]
async fn test_apply_product_creates_events_and_projects_to_graph() {
    let (app, event_log, projector) = test_server_with_projection().await;

    // 1. Apply a product with a container and volume
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "test-app", "version": "1.0.0", "description": "Test application" },
            { "type": "volume", "name": "data-vol", "product": "test-app", "size_gb": 50 },
            { "type": "container", "name": "web", "product": "test-app", "image": "test:1.0.0", "identity": "web-worker" }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&resource_file).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Should have 3 results
    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r["status"] == "declared"));

    // Verify correlation ID is present
    assert!(json.get("correlationId").is_some());

    // 2. Verify events were stored
    assert_eq!(event_log.len().await, 3);

    // 3. Give the projection loop time to process
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 4. Query the graph for the product
    let result = projector
        .query("SELECT ?product ?version WHERE { ?product <https://picloud.local/ontology#version> ?version }")
        .await
        .unwrap();

    assert_eq!(result.bindings.len(), 1);
    assert_eq!(
        result.bindings[0]["version"]["value"].as_str().unwrap(),
        "1.0.0"
    );

    // 5. Query for resources
    let result = projector
        .query("SELECT ?res ?rtype WHERE { ?res <https://picloud.local/ontology#resourceType> ?rtype }")
        .await
        .unwrap();

    // Should have volume + container (product uses ProductDeployed, not ResourceDeclared)
    let resource_types: Vec<&str> = result
        .bindings
        .iter()
        .filter_map(|r| r["rtype"]["value"].as_str())
        .collect();
    assert!(resource_types.contains(&"Volume"));
    assert!(resource_types.contains(&"Container"));
}

#[tokio::test]
async fn test_apply_invalid_resource_file_returns_400() {
    let (app, _, _) = test_server_with_deps();

    // Missing product reference
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "volume", "name": "orphan-vol", "product": "nonexistent", "size_gb": 10 }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&resource_file).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_cluster_graph_query() {
    let (app, _event_log, projector) = test_server_with_projection().await;

    // Apply a product
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "query-test", "version": "2.0.0" }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&resource_file).unwrap()))
        .unwrap();

    app.clone().oneshot(req).await.unwrap();

    // Give projection loop time to process
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Query via the /graph endpoint
    let req = Request::builder()
        .uri("/graph?query=SELECT%20%3Fp%20%3Fv%20WHERE%20%7B%20%3Fp%20%3Chttps%3A%2F%2Fpicloud.local%2Fontology%23name%3E%20%3Fv%20%7D")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["type"], "SparqlResult");
    assert!(!json["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_health_endpoint() {
    let (app, _, _) = test_server_with_deps();

    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// --- Fake implementations for testing ---

use async_trait::async_trait;

struct FakeCluster;
#[async_trait]
impl picloud_domain::traits::ClusterMembership for FakeCluster {
    async fn is_leader(&self) -> bool { true }
    async fn leader_id(&self) -> picloud_domain::error::Result<uuid::Uuid> {
        Ok(uuid::Uuid::nil())
    }
    async fn members(&self) -> picloud_domain::error::Result<Vec<picloud_domain::traits::NodeInfo>> {
        Ok(vec![])
    }
    async fn local_node_id(&self) -> uuid::Uuid { uuid::Uuid::nil() }
}

struct FakeIam;
#[async_trait]
impl picloud_domain::traits::IdentityProvider for FakeIam {
    async fn issue_token(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
        _product: Option<&str>,
    ) -> picloud_domain::error::Result<String> {
        Ok("fake-token".to_string())
    }
    async fn validate_token(&self, _token: &str) -> picloud_domain::error::Result<picloud_domain::traits::ValidatedIdentity> {
        Err(picloud_domain::error::PiCloudError::Unauthenticated)
    }
    async fn issue_workload_certificate(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<picloud_domain::traits::WorkloadCertificate> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
}

struct FakeStorage;
#[async_trait]
impl picloud_domain::traits::StorageBackend for FakeStorage {
    async fn allocate_volume(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
        _size: u64,
        _intent: &picloud_domain::storage::StorageIntent,
    ) -> picloud_domain::error::Result<picloud_domain::traits::VolumeHandle> {
        Ok(picloud_domain::traits::VolumeHandle {
            volume_iri: picloud_domain::iri::ResourceIri::new("https://picloud.local/test/vol").unwrap(),
            device_path: "/dev/test".to_string(),
            replicated_to: vec![],
        })
    }
    async fn delete_volume(&self, _iri: &picloud_domain::iri::ResourceIri) -> picloud_domain::error::Result<()> {
        Ok(())
    }
    async fn available_capacity_gb(&self) -> picloud_domain::error::Result<u64> {
        Ok(1000)
    }
}

struct FakeScheduler;
#[async_trait]
impl picloud_domain::traits::WorkloadScheduler for FakeScheduler {
    async fn schedule(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
        _spec: &picloud_domain::traits::WorkloadSpec,
    ) -> picloud_domain::error::Result<picloud_domain::traits::WorkloadHandle> {
        Ok(picloud_domain::traits::WorkloadHandle {
            workload_iri: picloud_domain::iri::ResourceIri::new("https://picloud.local/test/wl").unwrap(),
            node_id: uuid::Uuid::nil(),
            pid: Some(12345),
        })
    }
    async fn stop(&self, _iri: &picloud_domain::iri::ResourceIri) -> picloud_domain::error::Result<()> {
        Ok(())
    }
    async fn status(&self, _iri: &picloud_domain::iri::ResourceIri) -> picloud_domain::error::Result<picloud_domain::traits::WorkloadStatus> {
        Ok(picloud_domain::traits::WorkloadStatus::Running)
    }
}
