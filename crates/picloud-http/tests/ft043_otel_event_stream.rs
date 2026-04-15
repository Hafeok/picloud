//! FT-043 — OTel event stream — in-process pub/sub for traces, metrics, logs
//!
//! Covers TC-256, TC-313.
//! These tests verify that:
//! 1. The OtelStream in-process pub/sub delivers spans, metrics, and logs to subscribers
//! 2. Multiple subscribers each receive a copy of every datum
//! 3. Batch publish methods work correctly
//! 4. End-to-end: traces posted via /otel HTTP endpoint are delivered to in-process subscribers

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use chrono::Utc;
use tower::util::ServiceExt;

use picloud_domain::events::{MetricRecord, SpanRecord};
use picloud_domain::iri::ClusterDomain;
use picloud_events::InMemoryEventLog;
use picloud_http::{JsonlTelemetryStore, OtelDatum, OtelLogRecord, OtelStream, PiCloudHttpServer};
use picloud_rdf::OxigraphProjector;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

/// Build a test server, returning the router AND the OtelStream so tests can subscribe.
fn test_server_with_shared_otel(
) -> (axum::Router, Arc<OtelStream>, Arc<InMemoryEventLog>) {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let otel_stream = Arc::new(OtelStream::new(4096));
    let telemetry_dir =
        std::env::temp_dir().join(format!("picloud-otel-ft043-{}", uuid::Uuid::new_v4()));
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
        .with_otel(otel_stream.clone(), telemetry_store);

    (server.build_router(), otel_stream, event_log)
}

// ---------------------------------------------------------------------------
// Fakes (same as ft042_otlp_endpoint.rs)
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

fn make_span(trace_id: &str, span_id: &str, operation: &str, service: &str) -> SpanRecord {
    SpanRecord {
        trace_id: trace_id.to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        operation_name: operation.to_string(),
        service_name: service.to_string(),
        start_time: Utc::now(),
        end_time: Utc::now(),
        duration_ms: 42,
        status: "OK".to_string(),
        attributes: serde_json::json!({}),
    }
}

fn make_metric(name: &str, value: f64, service: &str) -> MetricRecord {
    MetricRecord {
        name: name.to_string(),
        value,
        unit: "ms".to_string(),
        metric_type: "gauge".to_string(),
        service_name: service.to_string(),
        timestamp: Utc::now(),
        attributes: serde_json::json!({}),
    }
}

fn make_log(severity: &str, body: &str, service: &str) -> OtelLogRecord {
    OtelLogRecord {
        timestamp: Utc::now(),
        severity: severity.to_string(),
        body: body.to_string(),
        service_name: service.to_string(),
        attributes: serde_json::json!({}),
    }
}

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

// ===========================================================================
// TC-256 — OTel event stream delivers traces to in-process subscriber
// ===========================================================================

/// TC-256: Scenario test — the OtelStream in-process pub/sub delivers traces,
/// metrics, and logs to subscribers.
///
/// Steps:
/// 1. Create an OtelStream and subscribe to it
/// 2. Publish a span — verify the subscriber receives it with correct trace_id
/// 3. Publish a metric — verify the subscriber receives it with correct name
/// 4. Publish a log — verify the subscriber receives it with correct body
/// 5. Verify multiple subscribers each receive a copy of every datum
/// 6. Verify batch publish_spans delivers all spans to subscriber
/// 7. Verify publishing with no subscribers does not panic
#[tokio::test]
async fn tc256_otel_event_stream_delivers_traces_to_in_process_subscriber() {
    // Step 1: Create OtelStream and subscribe
    let stream = Arc::new(OtelStream::new(256));
    let mut rx = stream.subscribe();

    // Step 2: Publish a span and verify delivery
    let span = make_span("trace-256-001", "span-001", "GET /api/products", "api-server");
    stream.publish(OtelDatum::Span(span.clone()));

    let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("Timed out waiting for span")
        .expect("Failed to receive span");

    match received {
        OtelDatum::Span(s) => {
            assert_eq!(s.trace_id, "trace-256-001", "Span trace_id should match");
            assert_eq!(s.span_id, "span-001", "Span span_id should match");
            assert_eq!(
                s.operation_name, "GET /api/products",
                "Span operation should match"
            );
            assert_eq!(s.service_name, "api-server", "Span service should match");
        }
        other => panic!("Expected OtelDatum::Span, got {:?}", other),
    }

    // Step 3: Publish a metric and verify delivery
    let metric = make_metric("http_request_duration_ms", 42.5, "api-server");
    stream.publish(OtelDatum::Metric(metric.clone()));

    let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("Timed out waiting for metric")
        .expect("Failed to receive metric");

    match received {
        OtelDatum::Metric(m) => {
            assert_eq!(
                m.name, "http_request_duration_ms",
                "Metric name should match"
            );
            assert_eq!(m.value, 42.5, "Metric value should match");
            assert_eq!(m.service_name, "api-server", "Metric service should match");
        }
        other => panic!("Expected OtelDatum::Metric, got {:?}", other),
    }

    // Step 4: Publish a log and verify delivery
    let log = make_log("INFO", "Request processed successfully", "api-server");
    stream.publish(OtelDatum::Log(log.clone()));

    let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("Timed out waiting for log")
        .expect("Failed to receive log");

    match received {
        OtelDatum::Log(l) => {
            assert_eq!(l.severity, "INFO", "Log severity should match");
            assert_eq!(
                l.body, "Request processed successfully",
                "Log body should match"
            );
            assert_eq!(l.service_name, "api-server", "Log service should match");
        }
        other => panic!("Expected OtelDatum::Log, got {:?}", other),
    }

    // Step 5: Multiple subscribers each receive a copy
    let mut rx2 = stream.subscribe();
    let mut rx3 = stream.subscribe();

    let multi_span = make_span("trace-256-multi", "span-multi", "POST /orders", "order-svc");
    stream.publish(OtelDatum::Span(multi_span.clone()));

    // All three subscribers should receive the span (rx still active from before)
    for (i, receiver) in [&mut rx, &mut rx2, &mut rx3].iter_mut().enumerate() {
        let received = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
            .await
            .unwrap_or_else(|_| panic!("Subscriber {i} timed out"))
            .unwrap_or_else(|e| panic!("Subscriber {i} recv failed: {e}"));

        match received {
            OtelDatum::Span(s) => {
                assert_eq!(
                    s.trace_id, "trace-256-multi",
                    "Subscriber {i} should receive correct trace_id"
                );
            }
            other => panic!("Subscriber {i} expected Span, got {:?}", other),
        }
    }

    // Step 6: Batch publish_spans delivers all spans
    let spans = vec![
        make_span("trace-batch-1", "sb-1", "op1", "svc1"),
        make_span("trace-batch-2", "sb-2", "op2", "svc2"),
        make_span("trace-batch-3", "sb-3", "op3", "svc3"),
    ];
    stream.publish_spans(&spans);

    let mut received_trace_ids = Vec::new();
    for _ in 0..3 {
        let received = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("Timed out waiting for batch span")
            .expect("Failed to receive batch span");

        match received {
            OtelDatum::Span(s) => received_trace_ids.push(s.trace_id),
            other => panic!("Expected Span in batch, got {:?}", other),
        }
    }
    assert_eq!(
        received_trace_ids,
        vec!["trace-batch-1", "trace-batch-2", "trace-batch-3"],
        "Batch spans should arrive in order"
    );

    // Step 7: Publishing with no subscribers does not panic
    let isolated_stream = OtelStream::new(16);
    // No subscribers — this should silently drop the datum
    isolated_stream.publish(OtelDatum::Span(make_span("no-sub", "ns-1", "op", "svc")));
    isolated_stream.publish_spans(&[make_span("no-sub2", "ns-2", "op", "svc")]);
    isolated_stream.publish_metrics(&[make_metric("m1", 1.0, "svc")]);
    // If we get here without panic, the test passes
}

// ===========================================================================
// TC-313 — OTel stream exit — traces delivered to in-process subscriber
// ===========================================================================

/// TC-313: Exit-criteria test — end-to-end verification that OTLP traces
/// posted via the HTTP endpoint are delivered to in-process OtelStream subscribers.
///
/// This validates the full path: HTTP POST /otel → OtelStream.publish → subscriber.recv
///
/// Steps:
/// 1. Build the HTTP server with a shared OtelStream reference
/// 2. Subscribe to the OtelStream before posting any data
/// 3. POST OTLP traces via /otel endpoint — verify HTTP 200
/// 4. Receive the published spans from the in-process subscriber
/// 5. Verify trace_id, service_name, and operation_name match the posted data
/// 6. POST metrics via /otel — verify the subscriber also receives metrics
/// 7. POST a mixed payload (spans + metrics + logs) and verify all types arrive
#[tokio::test]
async fn tc313_otel_stream_exit_traces_delivered_to_in_process_subscriber() {
    let (app, otel_stream, _event_log) = test_server_with_shared_otel();

    // Step 2: Subscribe to the OtelStream BEFORE posting data
    let mut rx = otel_stream.subscribe();

    // Step 3: POST OTLP traces via /otel endpoint
    let trace_payload = serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": { "stringValue": "tc313-payment-svc" }
                }]
            },
            "scopeSpans": [{
                "scope": { "name": "tc313-test" },
                "spans": [
                    {
                        "traceId": "00000000000000000000000000tc0313",
                        "spanId": "00000000tc313001",
                        "name": "POST /payments",
                        "kind": 1,
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano": "1700000002000000000",
                        "status": { "code": 1 },
                        "attributes": [{
                            "key": "http.method",
                            "value": { "stringValue": "POST" }
                        }]
                    },
                    {
                        "traceId": "00000000000000000000000000tc0313",
                        "spanId": "00000000tc313002",
                        "parentSpanId": "00000000tc313001",
                        "name": "db.insert",
                        "kind": 3,
                        "startTimeUnixNano": "1700000000500000000",
                        "endTimeUnixNano": "1700000001500000000",
                        "status": { "code": 1 }
                    }
                ]
            }]
        }]
    });

    let (status, body) = post_json(&app, "/otel", trace_payload).await;
    assert_eq!(status, StatusCode::OK, "POST /otel should return 200");
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(2),
        "Should accept 2 spans"
    );

    // Step 4-5: Receive the published spans and verify content
    let mut received_spans = Vec::new();
    for _ in 0..2 {
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("Timed out waiting for span from OtelStream")
            .expect("Failed to receive span from OtelStream");

        match received {
            OtelDatum::Span(s) => received_spans.push(s),
            other => panic!("Expected OtelDatum::Span from stream, got {:?}", other),
        }
    }

    assert_eq!(received_spans.len(), 2, "Should receive exactly 2 spans");

    // Verify the first span (POST /payments)
    let payment_span = received_spans
        .iter()
        .find(|s| s.operation_name == "POST /payments")
        .expect("Should find the POST /payments span");
    assert_eq!(
        payment_span.trace_id, "00000000000000000000000000tc0313",
        "Trace ID should match"
    );
    assert_eq!(
        payment_span.span_id, "00000000tc313001",
        "Span ID should match"
    );
    assert_eq!(
        payment_span.service_name, "tc313-payment-svc",
        "Service name should be extracted from resource attributes"
    );

    // Verify the child span (db.insert)
    let db_span = received_spans
        .iter()
        .find(|s| s.operation_name == "db.insert")
        .expect("Should find the db.insert span");
    assert_eq!(
        db_span.trace_id, "00000000000000000000000000tc0313",
        "Child span trace_id should match parent"
    );
    assert_eq!(
        db_span.parent_span_id.as_deref(),
        Some("00000000tc313001"),
        "Child span should reference parent span_id"
    );
    assert_eq!(
        db_span.service_name, "tc313-payment-svc",
        "Child span service_name should match"
    );

    // Verify duration is computed (end - start = 1000ms for db.insert)
    assert_eq!(
        db_span.duration_ms, 1000,
        "Duration should be computed from start/end timestamps"
    );

    // Step 6: POST metrics via /otel and verify subscriber receives them
    let metrics_payload = serde_json::json!({
        "metrics": [{
            "name": "payment_processing_time_ms",
            "value": 250.0,
            "unit": "ms",
            "metric_type": "histogram",
            "service_name": "tc313-payment-svc",
            "timestamp": "2026-01-01T00:00:00Z",
            "attributes": {"endpoint": "/payments"}
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

    let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("Timed out waiting for metric from OtelStream")
        .expect("Failed to receive metric from OtelStream");

    match received {
        OtelDatum::Metric(m) => {
            assert_eq!(
                m.name, "payment_processing_time_ms",
                "Metric name should match"
            );
            assert_eq!(m.value, 250.0, "Metric value should match");
            assert_eq!(
                m.service_name, "tc313-payment-svc",
                "Metric service should match"
            );
        }
        other => panic!("Expected OtelDatum::Metric from stream, got {:?}", other),
    }

    // Step 7: POST a mixed payload (spans + metrics + logs) and verify all arrive
    let mixed_payload = serde_json::json!({
        "spans": [{
            "trace_id": "tc313-mixed-trace",
            "span_id": "tc313-mixed-span",
            "parent_span_id": null,
            "operation_name": "GET /status",
            "service_name": "tc313-status-svc",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:00:01Z",
            "duration_ms": 1000,
            "status": "OK",
            "attributes": {}
        }],
        "metrics": [{
            "name": "status_check_count",
            "value": 1.0,
            "unit": "count",
            "metric_type": "counter",
            "service_name": "tc313-status-svc",
            "timestamp": "2026-01-01T00:00:00Z",
            "attributes": {}
        }],
        "logs": [{
            "timestamp": "2026-01-01T00:00:00Z",
            "severity": "INFO",
            "body": "Status check completed",
            "service_name": "tc313-status-svc",
            "attributes": {}
        }]
    });

    let (status, body) = post_json(&app, "/otel", mixed_payload).await;
    assert_eq!(status, StatusCode::OK, "Mixed payload should return 200");
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(3),
        "Should accept 1 span + 1 metric + 1 log = 3"
    );

    // Collect all 3 items from the subscriber
    let mut got_span = false;
    let mut got_metric = false;
    let mut got_log = false;

    for _ in 0..3 {
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("Timed out waiting for mixed datum")
            .expect("Failed to receive mixed datum");

        match received {
            OtelDatum::Span(s) => {
                assert_eq!(s.trace_id, "tc313-mixed-trace");
                assert_eq!(s.operation_name, "GET /status");
                got_span = true;
            }
            OtelDatum::Metric(m) => {
                assert_eq!(m.name, "status_check_count");
                got_metric = true;
            }
            OtelDatum::Log(l) => {
                assert_eq!(l.body, "Status check completed");
                assert_eq!(l.severity, "INFO");
                got_log = true;
            }
        }
    }

    assert!(got_span, "Mixed payload should deliver span to subscriber");
    assert!(
        got_metric,
        "Mixed payload should deliver metric to subscriber"
    );
    assert!(got_log, "Mixed payload should deliver log to subscriber");
}
