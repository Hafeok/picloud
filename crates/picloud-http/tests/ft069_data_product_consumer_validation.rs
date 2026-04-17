//! FT-069 Integration Tests — `dataProducts` consumer dependency validation
//!
//! Covers:
//!   TC-202: data_product_consumer_blocked_without_product (scenario)
//!
//! Verifies that `POST /api/apply` rejects a consuming Product declaring a
//! `dataProducts` dependency on a data product that does not exist. The apply
//! must fail with a `DataProductNotFound` error, no `ProductDeployed` event
//! may be emitted for the consumer, and the consumer Product must not appear
//! in the RDF catalog.
//!
//! ADR-056 rule 6: *A consumer Product declaring `dataProducts` dependencies
//! fails `resource apply` if the referenced data product does not exist at
//! the required `minVersion`.*

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use async_trait::async_trait;

use picloud_domain::iri::ClusterDomain;
use picloud_domain::traits::{EventFilter, EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use picloud_rdf::OxigraphProjector;

// --- Fake slice implementations -------------------------------------------

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

/// Build a test server with real event log and projector, plus an async
/// projection loop so events land in the RDF catalog promptly.
async fn test_server() -> (
    axum::Router,
    Arc<InMemoryEventLog>,
    Arc<OxigraphProjector>,
) {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let server = PiCloudHttpServer::new("127.0.0.1:0".parse().unwrap(), domain)
        .with_dependencies(
            event_log.clone(),
            projector.clone(),
            Arc::new(FakeCluster),
            Arc::new(FakeIam),
            Arc::new(FakeStorage),
            Arc::new(FakeScheduler),
        );

    // Pump events into the projector so the ASK fallback sees recently declared
    // data products in the RDF catalog.
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

    (server.build_router(), event_log, projector)
}

/// TC-202: attempt to deploy `maps-app` with a `dataProducts` dependency on
/// `photo-app/photo-locations` when that data product does not exist. Assert
/// `resource apply` fails with a `DataProductNotFound` error and that the
/// consumer Product is not deployed — no `ProductDeployed` event is emitted
/// and no consumer resource lives in the RDF catalog.
#[tokio::test]
async fn data_product_consumer_blocked_without_product() {
    let (app, event_log, projector) = test_server().await;

    // Apply the consumer Product with a missing dataProducts dependency.
    let resource_file = serde_json::json!({
        "resources": [
            {
                "type": "product",
                "name": "maps-app",
                "version": "1.0.0",
                "dataProducts": [
                    { "source": "photo-app/photo-locations", "minVersion": "1.0.0" }
                ]
            }
        ]
    });

    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&resource_file).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "apply must fail because photo-locations does not exist"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let error_message = json["error"].as_str().unwrap_or("");
    assert!(
        error_message.contains("DataProductNotFound"),
        "error should mention DataProductNotFound — got: {error_message}"
    );
    assert!(
        error_message.contains("photo-app/photo-locations"),
        "error should name the missing source — got: {error_message}"
    );

    // Assert the consumer is NOT deployed: no ProductDeployed event for maps-app
    // must have been appended to the log.
    let events = event_log.events_since(0).await;
    let deployed_maps_app = events.iter().any(|e| {
        e.event_type == "ProductDeployed"
            && e.payload.get("product_name").and_then(|v| v.as_str()) == Some("maps-app")
    });
    assert!(
        !deployed_maps_app,
        "no ProductDeployed event must be emitted for maps-app when the dep is missing"
    );

    // And no partial state lands in the RDF catalog.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let ask = "ASK { \
        <https://picloud.local/products/maps-app> \
        <https://picloud.local/ontology#resourceType> \"Product\" \
    }";
    let result = projector.query(ask).await.unwrap();
    let exists = result
        .bindings
        .first()
        .and_then(|b| b.get("result"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(
        !exists,
        "maps-app must not appear in the RDF catalog when the apply is rejected"
    );
}

/// Positive companion: once the data product is declared (and projected), the
/// consumer can be applied successfully. This guards against over-eager
/// rejection on apply.
#[tokio::test]
async fn data_product_consumer_succeeds_when_dp_exists() {
    let (app, event_log, projector) = test_server().await;

    // Step 1: declare the producer + data product first.
    let producer = serde_json::json!({
        "resources": [
            { "type": "product", "name": "photo-app", "version": "1.0.0" },
            {
                "type": "data-domain",
                "name": "geospatial",
                "steward": "identity/alice",
                "sensitivity": "internal"
            },
            {
                "type": "data-product",
                "name": "photo-locations",
                "product": "photo-app",
                "domain": "geospatial",
                "version": "1.0.0",
                "shapes": "./photo-locations.shacl",
                "projection": "./photo-locations.rq",
                "freshness": {
                    "maxAge": "15m",
                    "triggers": ["PlaceResolved"]
                }
            }
        ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&producer).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "producer + data product apply must succeed"
    );

    // Let the projection catch up — the ASK fallback will then find the dp.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Step 2: declare the consumer with a dataProducts dep.
    let consumer = serde_json::json!({
        "resources": [
            {
                "type": "product",
                "name": "maps-app",
                "version": "1.0.0",
                "dataProducts": [
                    { "source": "photo-app/photo-locations", "minVersion": "1.0.0" }
                ]
            }
        ]
    });
    let req = Request::builder()
        .method("POST")
        .uri("/api/apply")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&consumer).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "consumer apply must succeed when dp exists"
    );

    // ProductDeployed for maps-app should be emitted.
    let events = event_log.events_since(0).await;
    let deployed_maps_app = events.iter().any(|e| {
        e.event_type == "ProductDeployed"
            && e.payload.get("product_name").and_then(|v| v.as_str()) == Some("maps-app")
    });
    assert!(deployed_maps_app, "maps-app should be deployed");

    // And the projector should see the product — the projector records a
    // Product as `picloud:Resource` with `picloud:resourceType "Product"`.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let ask = "ASK { \
        <https://picloud.local/products/maps-app> \
        <https://picloud.local/ontology#resourceType> \"Product\" \
    }";
    let result = projector.query(ask).await.unwrap();
    let exists = result
        .bindings
        .first()
        .and_then(|b| b.get("result"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(exists, "maps-app must be projected into the RDF catalog");
}
