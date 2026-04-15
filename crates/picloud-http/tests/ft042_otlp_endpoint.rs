//! FT-042 — OTLP endpoint at picloud.local/otel
//!
//! Covers TC-255, TC-312.
//! These tests verify that:
//! 1. The /otel endpoint accepts OTLP JSON traces and returns 200 with accepted count
//! 2. The /otel endpoint accepts OTLP JSON metrics and returns 200 with accepted count
//! 3. Mixed trace+metric payloads are accepted in a single request
//! 4. Standard OTLP format (resourceSpans with scopeSpans) is parsed correctly
//! 5. Ingested traces and metrics can be queried back via /telemetry/spans and /telemetry/metrics

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use picloud_domain::iri::ClusterDomain;
use picloud_events::InMemoryEventLog;
use picloud_http::{JsonlTelemetryStore, OtelStream, PiCloudHttpServer};
use picloud_rdf::OxigraphProjector;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

fn test_server_with_otel() -> (axum::Router, Arc<InMemoryEventLog>, Arc<OxigraphProjector>) {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let otel_stream = Arc::new(OtelStream::new(4096));
    let telemetry_dir =
        std::env::temp_dir().join(format!("picloud-otel-test-{}", uuid::Uuid::new_v4()));
    let telemetry_store = Arc::new(JsonlTelemetryStore::new(&telemetry_dir));

    let server = PiCloudHttpServer::new("127.0.0.1:0".parse().unwrap(), domain)
        .with_dependencies(
            event_log.clone(),
            projector.clone(),
            Arc::new(FakeCluster),
            Arc::new(FakeIam),
            Arc::new(FakeStorage),
            Arc::new(FakeScheduler),
        )
        .with_otel(otel_stream, telemetry_store);

    (server.build_router(), event_log, projector)
}

// ---------------------------------------------------------------------------
// Fakes (mirrors ft038_config_store.rs)
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
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
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
    async fn jwks(
        &self,
    ) -> picloud_domain::error::Result<picloud_domain::identity::JsonWebKeySet> {
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
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn begin_registration(
        &self,
        _identity_iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<(
        picloud_domain::identity::ChallengeId,
        picloud_domain::identity::RegistrationChallenge,
    )> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn complete_registration(
        &self,
        _challenge_id: &str,
        _response: picloud_domain::identity::RegistrationResponse,
    ) -> picloud_domain::error::Result<picloud_domain::identity::RegisteredPasskey> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn begin_authentication(
        &self,
        _identity_iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<(
        picloud_domain::identity::ChallengeId,
        picloud_domain::identity::AuthenticationChallenge,
    )> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn complete_authentication(
        &self,
        _challenge_id: &str,
        _response: picloud_domain::identity::AuthenticationResponse,
    ) -> picloud_domain::error::Result<String> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn enroll_with_token(
        &self,
        _token: &str,
    ) -> picloud_domain::error::Result<(
        picloud_domain::identity::ChallengeId,
        picloud_domain::identity::RegistrationChallenge,
    )> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn begin_device_flow(
        &self,
    ) -> picloud_domain::error::Result<picloud_domain::identity::DeviceFlowResponse> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn poll_device_flow(
        &self,
        _device_code: &str,
    ) -> picloud_domain::error::Result<picloud_domain::identity::DeviceFlowPollResult> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
    }
    async fn complete_device_flow(
        &self,
        _device_code: &str,
        _identity_iri: &picloud_domain::iri::ResourceIri,
    ) -> picloud_domain::error::Result<()> {
        Err(picloud_domain::error::PiCloudError::Internal(
            "not implemented".into(),
        ))
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
// Helpers
// ---------------------------------------------------------------------------

async fn post_json(
    app: &axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({}));
    (status, json)
}

async fn get_json(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .uri(uri)
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({}));
    (status, json)
}

// ===========================================================================
// TC-255 — OTLP endpoint accepts traces and metrics at picloud.local/otel
// ===========================================================================

/// TC-255: Scenario test — POST OTLP JSON traces and metrics to /otel.
///
/// Steps:
/// 1. POST a standard OTLP resourceSpans payload to /otel — expect 200 with accepted count
/// 2. POST a simplified metrics payload to /otel — expect 200 with accepted count
/// 3. POST a mixed payload (traces + metrics) — expect 200 with total accepted count
/// 4. POST an empty payload — expect 200 with accepted: 0
/// 5. Verify the endpoint handles the standard OTLP format (resourceSpans/scopeSpans)
#[tokio::test]
async fn tc255_otlp_endpoint_accepts_traces_and_metrics_at_picloud_local_otel() {
    let (app, _event_log, _projector) = test_server_with_otel();

    // Step 1: POST standard OTLP resourceSpans payload
    let trace_payload = serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": { "stringValue": "tc255-test-service" }
                }]
            },
            "scopeSpans": [{
                "scope": { "name": "tc255-test" },
                "spans": [
                    {
                        "traceId": "00000000000000000000000000tc0255",
                        "spanId": "00000000tc255001",
                        "name": "GET /api/products",
                        "kind": 1,
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano": "1700000001000000000",
                        "status": { "code": 1 }
                    },
                    {
                        "traceId": "00000000000000000000000000tc0255",
                        "spanId": "00000000tc255002",
                        "parentSpanId": "00000000tc255001",
                        "name": "db.query",
                        "kind": 3,
                        "startTimeUnixNano": "1700000000100000000",
                        "endTimeUnixNano": "1700000000900000000",
                        "status": { "code": 1 }
                    }
                ]
            }]
        }]
    });

    let (status, body) = post_json(&app, "/otel", trace_payload).await;
    assert_eq!(status, StatusCode::OK, "POST /otel traces should return 200");
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(2),
        "Should accept 2 spans from the OTLP payload"
    );

    // Step 2: POST simplified metrics payload
    let metrics_payload = serde_json::json!({
        "metrics": [
            {
                "name": "http_request_duration_ms",
                "value": 42.5,
                "unit": "ms",
                "metric_type": "gauge",
                "service_name": "tc255-test-service",
                "timestamp": "2026-01-01T00:00:00Z",
                "attributes": {"method": "GET", "path": "/api/products"}
            },
            {
                "name": "http_requests_total",
                "value": 1024.0,
                "unit": "count",
                "metric_type": "counter",
                "service_name": "tc255-test-service",
                "timestamp": "2026-01-01T00:00:00Z",
                "attributes": {"method": "GET"}
            }
        ]
    });

    let (status, body) = post_json(&app, "/otel", metrics_payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /otel metrics should return 200"
    );
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(2),
        "Should accept 2 metrics from the payload"
    );

    // Step 3: POST mixed payload (traces + metrics)
    let mixed_payload = serde_json::json!({
        "spans": [{
            "trace_id": "mixed-trace-001",
            "span_id": "mixed-span-001",
            "parent_span_id": null,
            "operation_name": "POST /orders",
            "service_name": "tc255-order-svc",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:00:01Z",
            "duration_ms": 1000,
            "status": "OK",
            "attributes": {}
        }],
        "metrics": [{
            "name": "order_count",
            "value": 1.0,
            "unit": "count",
            "metric_type": "counter",
            "service_name": "tc255-order-svc",
            "timestamp": "2026-01-01T00:00:00Z",
            "attributes": {}
        }]
    });

    let (status, body) = post_json(&app, "/otel", mixed_payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /otel mixed should return 200"
    );
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(2),
        "Should accept 1 span + 1 metric = 2 total"
    );

    // Step 4: POST empty payload — should return 200 with accepted: 0
    let empty_payload = serde_json::json!({});
    let (status, body) = post_json(&app, "/otel", empty_payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /otel empty should return 200"
    );
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(0),
        "Empty payload should accept 0 items"
    );
}

// ===========================================================================
// TC-312 — OTLP endpoint exit — traces and metrics accepted at /otel
// ===========================================================================

/// TC-312: Exit-criteria test — end-to-end trace and metric ingestion via /otel
/// with verification that data is queryable via /telemetry/spans and /telemetry/metrics.
///
/// Steps:
/// 1. POST OTLP traces (resourceSpans format) to /otel — verify accepted
/// 2. POST OTLP metrics to /otel — verify accepted
/// 3. Query /telemetry/spans — verify the ingested spans are returned
/// 4. Query /telemetry/metrics — verify the ingested metrics are returned
/// 5. Verify service_name filtering works on /telemetry/spans query
#[tokio::test]
async fn tc312_otlp_endpoint_exit_traces_and_metrics_accepted_at_otel() {
    let (app, _event_log, _projector) = test_server_with_otel();

    // Step 1: POST OTLP traces with resourceSpans format
    let trace_payload = serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": { "stringValue": "tc312-api-server" }
                }]
            },
            "scopeSpans": [{
                "scope": { "name": "tc312-test" },
                "spans": [{
                    "traceId": "00000000000000000000000000tc0312",
                    "spanId": "00000000tc312001",
                    "name": "GET /health",
                    "kind": 1,
                    "startTimeUnixNano": "1700000000000000000",
                    "endTimeUnixNano": "1700000001000000000",
                    "status": { "code": 1 },
                    "attributes": [{
                        "key": "http.method",
                        "value": { "stringValue": "GET" }
                    }]
                }]
            }]
        }]
    });

    let (status, body) = post_json(&app, "/otel", trace_payload).await;
    assert_eq!(status, StatusCode::OK, "Trace ingestion should return 200");
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(1),
        "Should accept 1 span"
    );

    // Step 2: POST metrics via simplified format
    let metrics_payload = serde_json::json!({
        "metrics": [{
            "name": "http_response_time_ms",
            "value": 55.0,
            "unit": "ms",
            "metric_type": "histogram",
            "service_name": "tc312-api-server",
            "timestamp": "2026-01-01T00:00:00Z",
            "attributes": {"endpoint": "/health"}
        }]
    });

    let (status, body) = post_json(&app, "/otel", metrics_payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Metric ingestion should return 200"
    );
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(1),
        "Should accept 1 metric"
    );

    // Step 3: Query /telemetry/spans — the spans were written directly by the handler
    // Use a wide time range to catch the ingested spans (timestamps are from the OTLP payload)
    let (status, body) = get_json(
        &app,
        "/telemetry/spans?from=2023-11-01T00:00:00Z&to=2024-01-01T00:00:00Z",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Span query should return 200"
    );
    let spans = body
        .get("spans")
        .and_then(|v| v.as_array())
        .expect("Response should have a spans array");
    assert!(
        !spans.is_empty(),
        "Should find at least one ingested span via /telemetry/spans"
    );

    // Verify the span has the expected service name
    let found_service = spans.iter().any(|s| {
        s.get("service_name")
            .and_then(|v| v.as_str())
            == Some("tc312-api-server")
    });
    assert!(
        found_service,
        "Should find span with service_name=tc312-api-server"
    );

    // Step 4: Query /telemetry/metrics
    let (status, body) = get_json(
        &app,
        "/telemetry/metrics?from=2025-12-31T00:00:00Z&to=2026-01-02T00:00:00Z",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Metric query should return 200"
    );
    let metrics = body
        .get("metrics")
        .and_then(|v| v.as_array())
        .expect("Response should have a metrics array");
    assert!(
        !metrics.is_empty(),
        "Should find at least one ingested metric via /telemetry/metrics"
    );

    // Verify the metric has the expected name
    let found_metric = metrics.iter().any(|m| {
        m.get("name").and_then(|v| v.as_str()) == Some("http_response_time_ms")
    });
    assert!(
        found_metric,
        "Should find metric with name=http_response_time_ms"
    );

    // Step 5: Verify service_name filtering on span query
    let (status, body) = get_json(
        &app,
        "/telemetry/spans?from=2023-11-01T00:00:00Z&to=2024-01-01T00:00:00Z&service=tc312-api-server",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Filtered span query should return 200");
    let filtered_spans = body
        .get("spans")
        .and_then(|v| v.as_array())
        .expect("Filtered response should have a spans array");
    assert!(
        !filtered_spans.is_empty(),
        "Filtered query should find spans for tc312-api-server"
    );
    for span in filtered_spans {
        assert_eq!(
            span.get("service_name").and_then(|v| v.as_str()),
            Some("tc312-api-server"),
            "All filtered spans should have matching service_name"
        );
    }
}
