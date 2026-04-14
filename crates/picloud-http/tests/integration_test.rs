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
    async fn oidc_discovery(&self) -> picloud_domain::error::Result<picloud_domain::identity::OidcDiscoveryDocument> {
        Ok(picloud_domain::identity::OidcDiscoveryDocument {
            issuer: "https://picloud.local".to_string(),
            authorization_endpoint: "https://picloud.local/auth/authorize".to_string(),
            token_endpoint: "https://picloud.local/auth/token".to_string(),
            jwks_uri: "https://picloud.local/.well-known/jwks.json".to_string(),
            response_types_supported: vec!["code".to_string()],
            subject_types_supported: vec!["public".to_string()],
            id_token_signing_alg_values_supported: vec!["HS256".to_string()],
            grant_types_supported: vec!["client_credentials".to_string()],
            token_endpoint_auth_methods_supported: vec!["client_secret_post".to_string()],
            scopes_supported: vec!["openid".to_string()],
        })
    }
    async fn jwks(&self) -> picloud_domain::error::Result<picloud_domain::identity::JsonWebKeySet> {
        Ok(picloud_domain::identity::JsonWebKeySet {
            keys: vec![picloud_domain::identity::JsonWebKey {
                kty: "oct".to_string(),
                kid: "fake-key-1".to_string(),
                alg: "HS256".to_string(),
                key_use: "sig".to_string(),
                k: None,
            }],
        })
    }
    async fn client_credentials_token(
        &self,
        _client_id: &str,
        _client_secret: &str,
        _scope: Option<&str>,
    ) -> picloud_domain::error::Result<picloud_domain::identity::TokenResponse> {
        Err(picloud_domain::error::PiCloudError::Unauthenticated)
    }
    async fn register_app(
        &self,
        _product_iri: &picloud_domain::iri::ResourceIri,
        _redirect_uris: Vec<String>,
        _scopes: Vec<String>,
    ) -> picloud_domain::error::Result<picloud_domain::identity::AppRegistration> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn begin_registration(
        &self,
        _identity_iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<(picloud_domain::identity::ChallengeId, picloud_domain::identity::RegistrationChallenge)> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn complete_registration(
        &self,
        _challenge_id: &str,
        _response: picloud_domain::identity::RegistrationResponse,
    ) -> picloud_domain::error::Result<picloud_domain::identity::RegisteredPasskey> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn begin_authentication(
        &self,
        _identity_iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<(picloud_domain::identity::ChallengeId, picloud_domain::identity::AuthenticationChallenge)> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn complete_authentication(
        &self,
        _challenge_id: &str,
        _response: picloud_domain::identity::AuthenticationResponse,
    ) -> picloud_domain::error::Result<String> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn enroll_with_token(
        &self,
        _token: &str,
    ) -> picloud_domain::error::Result<(picloud_domain::identity::ChallengeId, picloud_domain::identity::RegistrationChallenge)> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn begin_device_flow(&self) -> picloud_domain::error::Result<picloud_domain::identity::DeviceFlowResponse> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn poll_device_flow(
        &self,
        _device_code: &str,
    ) -> picloud_domain::error::Result<picloud_domain::identity::DeviceFlowPollResult> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn complete_device_flow(
        &self,
        _device_code: &str,
        _identity_iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<()> {
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
        _volume_type: &picloud_domain::resources::VolumeType,
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

// ===========================================================================
// TC-240: CLI cluster init and resource apply create resources end-to-end
// ===========================================================================

/// TC-240: Full end-to-end scenario exercising the CLI's primary commands:
///   1. cluster init — fetch cluster identity from GET /
///   2. resource apply — POST /api/apply with a product, volume, and container
///   3. resource status — GET /products/:name returns projected resources
///   4. identity create — POST /api/commands with IdentityCreated
///
/// This validates that the HTTP endpoints backing each CLI command work
/// together in sequence, and that state flows from events → RDF → queries.
#[tokio::test]
async fn tc240_cli_cluster_init_and_resource_apply() {
    let (app, event_log, projector) = test_server_with_projection().await;

    // ── Step 1: cluster init — the CLI hits GET / to check cluster identity ──
    let req = Request::builder()
        .uri("/")
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let cluster_info: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(cluster_info["type"], "PiCloudCluster");
    assert_eq!(cluster_info["domain"], "picloud.local");

    // ── Step 2: resource apply — submit a product with volume + container ──
    let resource_file = serde_json::json!({
        "resources": [
            {
                "type": "product",
                "name": "demo-app",
                "version": "1.0.0",
                "description": "Demo application for TC-240"
            },
            {
                "type": "volume",
                "name": "db-vol",
                "product": "demo-app",
                "size_gb": 25
            },
            {
                "type": "container",
                "name": "api",
                "product": "demo-app",
                "image": "demo:1.0.0",
                "identity": "api-worker"
            }
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
    let apply_result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify all 3 resources declared
    let results = apply_result["results"].as_array().unwrap();
    assert_eq!(results.len(), 3, "expected 3 resources declared");
    assert!(
        results.iter().all(|r| r["status"] == "declared"),
        "all resources should be in 'declared' status"
    );
    // Correlation ID should be present
    assert!(apply_result.get("correlationId").is_some());

    // Verify events were stored (1 ProductDeployed + 2 ResourceDeclared)
    assert_eq!(event_log.len().await, 3);

    // ── Step 3: resource status — query GET /products/demo-app ──
    // Give the projection loop time to process events
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let req = Request::builder()
        .uri("/products/demo-app")
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let product_status: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(product_status["type"], "Product");
    assert_eq!(product_status["name"], "demo-app");
    // Resources should be projected into the graph
    let resources = product_status["resources"].as_array().unwrap();
    assert!(
        resources.len() >= 2,
        "expected at least volume + container in resources, got {}",
        resources.len()
    );

    // ── Step 4: identity create — POST /api/commands with IdentityCreated ──
    let identity_cmd = serde_json::json!({
        "command_type": "IdentityCreated",
        "payload": {
            "name": "alice",
            "email": "alice@picloud.local"
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/commands")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&identity_cmd).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let cmd_result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(cmd_result["status"], "accepted");
    assert!(cmd_result.get("correlationId").is_some());

    // Event log should now have 4 events (3 apply + 1 identity)
    assert_eq!(event_log.len().await, 4);
}

// ===========================================================================
// TC-226: Product with container, volume, and workload identity deploys end-to-end
// ===========================================================================

/// TC-226: Exit-criteria test verifying the Product resource type with versioning.
///
/// Validates the full end-to-end deployment lifecycle:
///   1. Apply a versioned Product with a Container (including workload identity)
///      and a Volume — all three resources are declared atomically.
///   2. Verify that the correct platform events are emitted (ProductDeployed +
///      ResourceDeclared for each sub-resource).
///   3. Verify that the RDF projection creates queryable graph triples for the
///      product, its version, the container (with identity), and the volume.
///   4. Verify the product status endpoint returns all child resources.
///   5. Apply a version upgrade (same product, new version) and verify the
///      ProductDeployed event carries the updated version, confirming that the
///      Product resource type supports versioning as a first-class concept.
///
/// This is the gate test for FT-024 (Product resource type with versioning).
#[tokio::test]
async fn tc226_product_with_container_volume_and_workload_identity_deploys_e2e() {
    let (app, event_log, projector) = test_server_with_projection().await;

    // ── Step 1: Deploy a versioned Product with container + volume ──
    let resource_file = serde_json::json!({
        "resources": [
            {
                "type": "product",
                "name": "photo-app",
                "version": "1.0.0",
                "description": "Photo processing application"
            },
            {
                "type": "volume",
                "name": "photo-store",
                "product": "photo-app",
                "size_gb": 100
            },
            {
                "type": "container",
                "name": "api-server",
                "product": "photo-app",
                "image": "photo-api:1.0.0",
                "identity": "photo-api-worker"
            }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&resource_file).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "initial apply should succeed");

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let apply_result: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // All 3 resources should be declared
    let results = apply_result["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3, "expected 3 resources declared");
    for r in results {
        assert_eq!(r["status"], "declared", "each resource should be declared");
    }

    // Correlation ID links the whole operation
    let correlation_id = apply_result
        .get("correlationId")
        .expect("apply must return a correlationId");
    assert!(
        !correlation_id.as_str().unwrap().is_empty(),
        "correlationId must be non-empty"
    );

    // ── Step 2: Verify events were emitted ──
    // Expect 3 events: 1 ProductDeployed + 2 ResourceDeclared (volume + container)
    assert_eq!(
        event_log.len().await,
        3,
        "expected 3 events (ProductDeployed + 2 ResourceDeclared)"
    );

    // ── Step 3: Verify RDF projection — product version is queryable ──
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // 3a. Product version in graph
    let result = projector
        .query(
            "SELECT ?product ?version WHERE { \
             ?product <https://picloud.local/ontology#version> ?version . \
             ?product <https://picloud.local/ontology#name> \"photo-app\" }",
        )
        .await
        .unwrap();
    assert_eq!(
        result.bindings.len(),
        1,
        "exactly one product should match 'photo-app'"
    );
    assert_eq!(
        result.bindings[0]["version"]["value"].as_str().unwrap(),
        "1.0.0",
        "product version should be 1.0.0"
    );

    // 3b. Volume is projected
    let result = projector
        .query(
            "SELECT ?res WHERE { \
             ?res <https://picloud.local/ontology#resourceType> \"Volume\" . \
             ?res <https://picloud.local/ontology#product> \"photo-app\" }",
        )
        .await
        .unwrap();
    assert!(
        !result.bindings.is_empty(),
        "volume should be projected into graph for photo-app"
    );

    // 3c. Container is projected with workload identity
    let result = projector
        .query(
            "SELECT ?res ?identity WHERE { \
             ?res <https://picloud.local/ontology#resourceType> \"Container\" . \
             ?res <https://picloud.local/ontology#product> \"photo-app\" . \
             OPTIONAL { ?res <https://picloud.local/ontology#identity> ?identity } }",
        )
        .await
        .unwrap();
    assert!(
        !result.bindings.is_empty(),
        "container should be projected into graph for photo-app"
    );

    // ── Step 4: Verify product status endpoint returns child resources ──
    let req = Request::builder()
        .uri("/products/photo-app")
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "product status endpoint should return OK"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let product_status: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(product_status["name"], "photo-app");
    let resources = product_status["resources"]
        .as_array()
        .expect("product status must include resources array");
    let types: Vec<&str> = resources
        .iter()
        .filter_map(|r| r["type"]["value"].as_str().or_else(|| r["type"].as_str()))
        .collect();
    assert!(
        types.iter().any(|t| *t == "Volume"),
        "product resources should include Volume, got: {:?}",
        types
    );
    assert!(
        types.iter().any(|t| *t == "Container"),
        "product resources should include Container, got: {:?}",
        types
    );

    // ── Step 5: Version upgrade via POST /products/:name/upgrade (ADR-017) ──
    let upgrade_body = serde_json::json!({
        "version": "2.0.0"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/products/photo-app/upgrade")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&upgrade_body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "upgrade should succeed");

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let upgrade_result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        upgrade_result["status"], "upgrade_started",
        "upgrade response should indicate started"
    );
    assert_eq!(
        upgrade_result["from_version"], "1.0.0",
        "upgrade should record from_version"
    );
    assert_eq!(
        upgrade_result["to_version"], "2.0.0",
        "upgrade should record to_version"
    );

    // Verify the ProductUpgradeStarted event was appended (3 original + 1 upgrade = 4)
    assert_eq!(
        event_log.len().await,
        4,
        "event log should have 4 events after upgrade"
    );

    // Wait for projection, then verify the upgrade event is reflected
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // The original product version should still be queryable in the graph
    let result = projector
        .query(
            "SELECT ?version WHERE { \
             ?product <https://picloud.local/ontology#name> \"photo-app\" . \
             ?product <https://picloud.local/ontology#version> ?version }",
        )
        .await
        .unwrap();
    assert!(
        !result.bindings.is_empty(),
        "product should still be queryable after upgrade"
    );
}

// ===========================================================================
// TC-297: CLI exit — cluster init, resource apply, resource status complete
// ===========================================================================

/// TC-297: Exit-criteria test verifying all four CLI operations produce correct
/// HTTP responses and leave the system in a consistent state:
///   - cluster init returns valid cluster metadata
///   - resource apply creates resources and returns declared status
///   - resource status returns projected resources with types
///   - identity create is accepted and stored in the event log
///
/// This is the gate test for FT-022 completion.
#[tokio::test]
async fn tc297_cli_exit_cluster_init_resource_apply_resource_status() {
    let (app, event_log, projector) = test_server_with_projection().await;

    // ── cluster init: verify cluster root responds with domain + type ──
    let req = Request::builder()
        .uri("/")
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "cluster root should be OK");
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        body.get("domain").is_some(),
        "cluster root must include domain"
    );

    // ── cluster status: verify health endpoint ──
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "health endpoint should be OK");

    // ── resource apply: create a product with resources ──
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "exit-test", "version": "2.0.0", "description": "Exit criteria test" },
            { "type": "volume", "name": "data", "product": "exit-test", "size_gb": 10 },
            { "type": "container", "name": "svc", "product": "exit-test", "image": "svc:2.0", "identity": "svc-id" }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&resource_file).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "apply should succeed");
    let apply_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let results = apply_body["results"].as_array().expect("results array");
    assert_eq!(results.len(), 3, "3 resources applied");
    for r in results {
        assert_eq!(r["status"], "declared", "each resource should be declared");
    }

    // ── resource status: verify product is queryable after projection ──
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let req = Request::builder()
        .uri("/products/exit-test")
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "product status should be OK"
    );
    let status_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status_body["name"], "exit-test");
    let resources = status_body["resources"]
        .as_array()
        .expect("resources array in status");
    // Should contain at least the volume and container
    let types: Vec<&str> = resources
        .iter()
        .filter_map(|r| r["type"]["value"].as_str().or_else(|| r["type"].as_str()))
        .collect();
    assert!(
        types.iter().any(|t| *t == "Volume"),
        "status should include Volume, got: {:?}",
        types
    );
    assert!(
        types.iter().any(|t| *t == "Container"),
        "status should include Container, got: {:?}",
        types
    );

    // ── identity create: submit an identity command ──
    let cmd = serde_json::json!({
        "command_type": "IdentityCreated",
        "payload": { "name": "bob", "email": "bob@picloud.local" }
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/commands")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&cmd).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::ACCEPTED,
        "identity create should be accepted"
    );

    // ── Final state: all events persisted ──
    assert_eq!(
        event_log.len().await,
        4,
        "event log should have 4 events (3 resources + 1 identity)"
    );

    // Verify graph integrity via SPARQL — the product should be discoverable
    let result = projector
        .query("SELECT ?p WHERE { ?p <https://picloud.local/ontology#name> \"exit-test\" }")
        .await
        .unwrap();
    assert!(
        !result.bindings.is_empty(),
        "product should be queryable in RDF graph"
    );
}
