//! TC-350 Regression Test — Telemetry SQL query endpoint returns 200 on simple
//! parquet read.
//!
//! Guards against the failure observed on the Pi 5 cluster (2026-04-17) where
//! the `parquet-write-read` E2E scenario reported `telemetry SQL query
//! endpoint returned status 500 Internal Server Error`.
//!
//! **Invariant under test:** after the platform has accepted OTel data and
//! written at least one Parquet partition, a simple SQL query through the
//! telemetry endpoint (`/api/telemetry/query`) returns HTTP 200 with valid
//! rows — never 500.
//!
//! Shape of the test (mirrors the shape described in the TC front-matter):
//!   1. Spin up an in-process `picloud-server` router backed by a
//!      `ParquetTelemetryStore` in a temp dir (single node).
//!   2. Accept a batch of OTel metric points via the `/otel` ingestion
//!      endpoint.
//!   3. Wait for the direct-write path to flush to Parquet.
//!   4. Issue a trivial SQL query (`SELECT COUNT(*) AS total FROM metrics`)
//!      via the DataFusion SQL endpoint at `/api/telemetry/query`.
//!   5. Assert HTTP 200 and at least one row. Non-200 or an error body must
//!      fail the test; the response body is captured verbatim on failure so
//!      CI logs surface the root cause.
//!
//! A second assertion covers the even more trivial case of
//! `SELECT COUNT(*) FROM traces` on a store that has only metrics — this is
//! the exact query the Pi 5 E2E scenario used, and historically returned
//! 500 when DataFusion received an empty partition vector.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use picloud_domain::iri::ClusterDomain;
use picloud_events::InMemoryEventLog;
use picloud_http::{ParquetTelemetryStore, OtelStream, PiCloudHttpServer};
use picloud_rdf::OxigraphProjector;

// ---------------------------------------------------------------------------
// Test infrastructure — single-node server with ParquetTelemetryStore
// ---------------------------------------------------------------------------

fn test_server_with_parquet_store() -> (axum::Router, std::path::PathBuf) {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let otel_stream = Arc::new(OtelStream::new(4096));
    let telemetry_dir = std::env::temp_dir().join(format!(
        "picloud-tc350-{}",
        uuid::Uuid::new_v4()
    ));
    let telemetry_store = Arc::new(ParquetTelemetryStore::new(&telemetry_dir));

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

    (server.build_router(), telemetry_dir)
}

// ---------------------------------------------------------------------------
// Fakes — minimal trait impls (mirrors ft042_otlp_endpoint.rs)
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
// HTTP helpers
// ---------------------------------------------------------------------------

async fn post_json(
    app: &axum::Router,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, String, serde_json::Value) {
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
    let raw = String::from_utf8_lossy(&bytes).to_string();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({}));
    (status, raw, json)
}

// ===========================================================================
// TC-350 — Telemetry SQL query endpoint returns 200 on simple parquet read
// ===========================================================================

#[tokio::test]
async fn tc350_telemetry_sql_query_endpoint_returns_200() {
    let (app, telemetry_dir) = test_server_with_parquet_store();

    // -------------------------------------------------------------------
    // Step 1: Verify that a trivial SQL query on an **empty** store still
    //         returns HTTP 200 (this is the exact shape that produced the
    //         500 on the Pi 5 cluster — no data written yet, then query
    //         `SELECT COUNT(*) FROM traces`).
    // -------------------------------------------------------------------
    let empty_query = serde_json::json!({
        "signal": "traces",
        "sql":    "SELECT COUNT(*) AS total FROM traces",
    });

    let (status, raw, body) = post_json(&app, "/api/telemetry/query", empty_query).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "SQL query on empty parquet store must return 200, got {status}. Body: {raw}"
    );
    assert!(
        body.get("error").is_none(),
        "Response body must not contain an error field on a successful query. Body: {raw}"
    );
    let empty_rows = body
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        empty_rows.len(),
        1,
        "COUNT(*) on an empty table must return exactly one row (the count). Body: {raw}"
    );
    // The count on an empty store must be 0 — surface the row for diagnostics.
    let empty_count = empty_rows[0]
        .get("total")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    assert_eq!(
        empty_count, 0,
        "COUNT(*) on an empty store must be 0, got {empty_count}. Body: {raw}"
    );

    // -------------------------------------------------------------------
    // Step 2: Ingest a batch of OTel metric points via the /otel endpoint.
    // -------------------------------------------------------------------
    let metrics_payload = serde_json::json!({
        "metrics": [
            {
                "name": "http_request_duration_ms",
                "value": 42.5,
                "unit": "ms",
                "metric_type": "gauge",
                "service_name": "tc350-test-service",
                "timestamp": "2026-04-17T13:00:00Z",
                "attributes": {"method": "GET"}
            },
            {
                "name": "http_requests_total",
                "value": 1024.0,
                "unit": "count",
                "metric_type": "counter",
                "service_name": "tc350-test-service",
                "timestamp": "2026-04-17T13:00:01Z",
                "attributes": {"method": "GET"}
            },
            {
                "name": "cpu_usage_pct",
                "value": 65.3,
                "unit": "percent",
                "metric_type": "gauge",
                "service_name": "tc350-test-service",
                "timestamp": "2026-04-17T13:00:02Z",
                "attributes": {}
            }
        ]
    });

    let (status, raw, body) = post_json(&app, "/otel", metrics_payload).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "OTel ingestion must return 200. Body: {raw}"
    );
    assert_eq!(
        body.get("accepted").and_then(|v| v.as_u64()),
        Some(3),
        "Should accept 3 metric points. Body: {raw}"
    );

    // -------------------------------------------------------------------
    // Step 3: Wait briefly for the direct-write path to persist the
    //         Parquet files. `handle_otel_ingest` writes synchronously to
    //         the store, so this is a small safety margin rather than a
    //         real poll.
    // -------------------------------------------------------------------
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Sanity check: at least one Parquet partition directory exists on disk.
    let metrics_dir = telemetry_dir.join("metrics");
    assert!(
        metrics_dir.exists(),
        "metrics/ directory must exist after ingestion: {}",
        metrics_dir.display()
    );
    let partitions: Vec<_> = std::fs::read_dir(&metrics_dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    assert!(
        !partitions.is_empty(),
        "At least one hourly partition must exist under {}",
        metrics_dir.display()
    );

    // -------------------------------------------------------------------
    // Step 4: Issue the trivial SQL query against the metrics table.
    // -------------------------------------------------------------------
    let query_body = serde_json::json!({
        "signal": "metrics",
        "sql":    "SELECT COUNT(*) AS total FROM metrics",
    });

    let (status, raw, body) = post_json(&app, "/api/telemetry/query", query_body).await;

    // -------------------------------------------------------------------
    // Step 5: Assertions — the invariant under test.
    // -------------------------------------------------------------------
    assert_eq!(
        status,
        StatusCode::OK,
        "Telemetry SQL query must return 200, got {status}. Body: {raw}"
    );
    assert!(
        body.get("error").is_none(),
        "Response body must not contain an error field. Body: {raw}"
    );

    let columns = body
        .get("columns")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !columns.is_empty(),
        "Response must include a non-empty columns list. Body: {raw}"
    );

    let rows = body
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !rows.is_empty(),
        "SELECT COUNT(*) must return at least one row. Body: {raw}"
    );

    // The aggregate result must reflect the 3 metric points we ingested.
    let total = rows[0]
        .get("total")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    assert_eq!(
        total, 3,
        "COUNT(*) must equal the 3 ingested metric points, got {total}. Body: {raw}"
    );

    // -------------------------------------------------------------------
    // Step 6: Also verify that a straight `SELECT *` returns rows.
    //         This catches any bug that only affects projection/aggregation.
    // -------------------------------------------------------------------
    let select_star = serde_json::json!({
        "signal": "metrics",
        "sql":    "SELECT * FROM metrics",
    });

    let (status, raw, body) = post_json(&app, "/api/telemetry/query", select_star).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "SELECT * FROM metrics must return 200. Body: {raw}"
    );
    let star_rows = body
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        star_rows.len(),
        3,
        "SELECT * FROM metrics must return 3 rows after 3 ingests. Body: {raw}"
    );

    // Cleanup the temp directory on success so CI does not accumulate state.
    let _ = std::fs::remove_dir_all(&telemetry_dir);
}
