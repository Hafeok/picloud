//! FT-079 — Product event schema IRIs served from platform HTTP layer
//!
//! Covers TC-279, TC-336.
//! These tests verify that:
//! 1. Platform event schema IRIs return valid JSON Schema documents via HTTP GET
//! 2. Product event schema IRIs return valid JSON Schema documents with product metadata
//! 3. Schema responses include all required EventEnvelope fields
//! 4. Content-Type is application/json
//! 5. Invalid versions return 400

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use picloud_domain::iri::ClusterDomain;
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use picloud_rdf::OxigraphProjector;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

fn test_server() -> axum::Router {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let server = PiCloudHttpServer::new("127.0.0.1:0".parse().unwrap(), domain)
        .with_dependencies(
            event_log,
            projector,
            Arc::new(FakeCluster),
            Arc::new(FakeIam),
            Arc::new(FakeStorage),
            Arc::new(FakeScheduler),
        );

    server.build_router()
}

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

struct FakeCluster;
#[async_trait]
impl picloud_domain::traits::ClusterMembership for FakeCluster {
    async fn is_leader(&self) -> bool {
        true
    }
    async fn leader_id(&self) -> picloud_domain::error::Result<uuid::Uuid> {
        Ok(uuid::Uuid::nil())
    }
    async fn members(
        &self,
    ) -> picloud_domain::error::Result<Vec<picloud_domain::traits::NodeInfo>> {
        Ok(vec![])
    }
    async fn local_node_id(&self) -> uuid::Uuid {
        uuid::Uuid::nil()
    }
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
    async fn validate_token(
        &self,
        _token: &str,
    ) -> picloud_domain::error::Result<picloud_domain::traits::ValidatedIdentity> {
        Err(picloud_domain::error::PiCloudError::Unauthenticated)
    }
    async fn issue_workload_certificate(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<picloud_domain::traits::WorkloadCertificate> {
        Err(picloud_domain::error::PiCloudError::Internal("not implemented".into()))
    }
    async fn oidc_discovery(
        &self,
    ) -> picloud_domain::error::Result<picloud_domain::identity::OidcDiscoveryDocument> {
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
            volume_iri: picloud_domain::iri::ResourceIri::new("https://picloud.local/test/vol")
                .unwrap(),
            device_path: "/dev/test".to_string(),
            replicated_to: vec![],
        })
    }
    async fn delete_volume(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<()> {
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
            workload_iri: picloud_domain::iri::ResourceIri::new("https://picloud.local/test/wl")
                .unwrap(),
            node_id: uuid::Uuid::nil(),
            pid: Some(12345),
        })
    }
    async fn stop(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<()> {
        Ok(())
    }
    async fn status(
        &self,
        _iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<picloud_domain::traits::WorkloadStatus> {
        Ok(picloud_domain::traits::WorkloadStatus::Running)
    }
}

// ---------------------------------------------------------------------------
// TC-279: Event schema IRI returns schema document via HTTP GET (scenario)
// ---------------------------------------------------------------------------

/// TC-279: Emit an event with a schema IRI, then GET that IRI and verify
/// the response is a valid JSON Schema document describing the event envelope.
///
/// Steps:
/// 1. GET the platform event schema IRI `/schemas/events/ResourceReady/v1`
/// 2. Assert HTTP 200 with Content-Type application/json
/// 3. Assert the response body is a valid JSON Schema with correct $id, title,
///    type, properties (all EventEnvelope fields), and required array
/// 4. GET a product event schema IRI `/products/photo-app/schemas/events/OrderPlaced/v1`
/// 5. Assert HTTP 200 with x-picloud-product extension field
/// 6. Assert invalid version returns 400
#[tokio::test]
async fn tc279_event_schema_iri_returns_schema_document_via_http_get() {
    let app = test_server();

    // --- Platform event schema ---
    let req = Request::builder()
        .uri("/schemas/events/ResourceReady/v1")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET platform event schema must return 200"
    );

    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("Content-Type header must be present")
        .to_str()
        .unwrap();
    assert!(
        ct.contains("application/json"),
        "Content-Type must be application/json, got: {ct}"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body)
        .expect("response body must be valid JSON");

    // Verify JSON Schema structure
    assert_eq!(
        json["$id"],
        "https://picloud.local/schemas/events/ResourceReady/v1",
        "$id must match the schema IRI"
    );
    assert_eq!(
        json["$schema"],
        "https://json-schema.org/draft/2020-12/schema",
        "$schema must reference JSON Schema 2020-12"
    );
    assert_eq!(json["title"], "ResourceReady");
    assert_eq!(json["type"], "object");

    // Verify all EventEnvelope fields are present in properties
    let props = &json["properties"];
    assert!(props["id"].is_object(), "must have id property");
    assert!(props["schema"].is_object(), "must have schema property");
    assert!(props["event_type"].is_object(), "must have event_type property");
    assert!(props["timestamp"].is_object(), "must have timestamp property");
    assert!(props["source"].is_object(), "must have source property");
    assert!(props["correlation_id"].is_object(), "must have correlation_id property");
    assert!(props["payload"].is_object(), "must have payload property");

    // Verify required array
    let required = json["required"]
        .as_array()
        .expect("required must be an array");
    let required_strs: Vec<&str> = required
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for field in &["id", "schema", "event_type", "timestamp", "source", "correlation_id", "payload"] {
        assert!(
            required_strs.contains(field),
            "required array must contain '{field}'"
        );
    }

    // --- Product event schema ---
    let req = Request::builder()
        .uri("/products/photo-app/schemas/events/OrderPlaced/v1")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET product event schema must return 200"
    );

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(
        json["$id"],
        "https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v1",
        "product schema $id must include product path"
    );
    assert_eq!(json["title"], "photo-app/OrderPlaced");
    assert_eq!(
        json["x-picloud-product"],
        "https://picloud.local/products/photo-app",
        "product schema must include x-picloud-product extension"
    );

    // --- Invalid version returns 400 ---
    let req = Request::builder()
        .uri("/schemas/events/ResourceReady/vabc")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "invalid version must return 400"
    );
}

// ---------------------------------------------------------------------------
// TC-336: Schema IRI exit — event schema served via HTTP GET (exit-criteria)
// ---------------------------------------------------------------------------

/// TC-336: Exit criteria — every schema IRI referenced by an EventEnvelope is
/// dereferenceable via HTTP GET and returns a valid JSON Schema.
///
/// This test verifies the complete contract:
/// 1. Multiple platform event types all resolve (ResourceReady, NodeJoined, ProductDeployed)
/// 2. Multiple product event types all resolve with correct product scoping
/// 3. Version parameter is respected (v1 vs v2 produce distinct $id values)
/// 4. JSON Schema draft reference is always present
/// 5. Content-Type is always application/json
#[tokio::test]
async fn tc336_schema_iri_exit_event_schema_served_via_http_get() {
    let app = test_server();

    // --- Verify multiple platform event types resolve ---
    let platform_events = vec![
        ("ResourceReady", 1),
        ("NodeJoined", 1),
        ("ProductDeployed", 1),
        ("LeaderElected", 1),
        ("ResourceFailed", 1),
    ];

    for (event_type, version) in &platform_events {
        let uri = format!("/schemas/events/{event_type}/v{version}");
        let req = Request::builder()
            .uri(&uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GET {uri} must return 200"
        );

        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            ct.contains("application/json"),
            "{uri}: Content-Type must be application/json"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let expected_id = format!(
            "https://picloud.local/schemas/events/{event_type}/v{version}"
        );
        assert_eq!(
            json["$id"], expected_id,
            "{uri}: $id must match the request IRI"
        );
        assert_eq!(
            json["$schema"],
            "https://json-schema.org/draft/2020-12/schema",
            "{uri}: $schema must be JSON Schema 2020-12"
        );
        assert_eq!(
            json["title"], *event_type,
            "{uri}: title must match event type"
        );
        assert_eq!(json["type"], "object");
    }

    // --- Verify product event schemas across different products ---
    let product_events = vec![
        ("photo-app", "OrderPlaced", 1),
        ("photo-app", "OrderShipped", 2),
        ("analytics", "PageViewed", 1),
    ];

    for (product, event_type, version) in &product_events {
        let uri = format!(
            "/products/{product}/schemas/events/{event_type}/v{version}"
        );
        let req = Request::builder()
            .uri(&uri)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "GET {uri} must return 200"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let expected_id = format!(
            "https://picloud.local/products/{product}/schemas/events/{event_type}/v{version}"
        );
        assert_eq!(json["$id"], expected_id, "{uri}: $id mismatch");

        let expected_product_iri =
            format!("https://picloud.local/products/{product}");
        assert_eq!(
            json["x-picloud-product"], expected_product_iri,
            "{uri}: x-picloud-product must point to product IRI"
        );

        // Product must be a const in the schema properties
        assert_eq!(
            json["properties"]["product"]["const"], *product,
            "{uri}: product property must have const constraint"
        );
    }

    // --- Version parameter produces distinct schemas ---
    let req_v1 = Request::builder()
        .uri("/schemas/events/ResourceReady/v1")
        .body(Body::empty())
        .unwrap();
    let resp_v1 = app.clone().oneshot(req_v1).await.unwrap();
    let body_v1 = axum::body::to_bytes(resp_v1.into_body(), usize::MAX)
        .await
        .unwrap();
    let json_v1: serde_json::Value = serde_json::from_slice(&body_v1).unwrap();

    let req_v2 = Request::builder()
        .uri("/schemas/events/ResourceReady/v2")
        .body(Body::empty())
        .unwrap();
    let resp_v2 = app.clone().oneshot(req_v2).await.unwrap();
    let body_v2 = axum::body::to_bytes(resp_v2.into_body(), usize::MAX)
        .await
        .unwrap();
    let json_v2: serde_json::Value = serde_json::from_slice(&body_v2).unwrap();

    assert_ne!(
        json_v1["$id"], json_v2["$id"],
        "v1 and v2 must produce distinct schema $id values"
    );
    assert_eq!(
        json_v1["$id"],
        "https://picloud.local/schemas/events/ResourceReady/v1"
    );
    assert_eq!(
        json_v2["$id"],
        "https://picloud.local/schemas/events/ResourceReady/v2"
    );

    // --- Invalid version returns 400 for both platform and product ---
    let req = Request::builder()
        .uri("/schemas/events/ResourceReady/vnotanumber")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let req = Request::builder()
        .uri("/products/photo-app/schemas/events/OrderPlaced/vxyz")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
