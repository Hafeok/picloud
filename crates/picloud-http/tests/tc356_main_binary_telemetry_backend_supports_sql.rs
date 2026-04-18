//! TC-356 — JsonlTelemetryStore used by main binary supports SQL queries.
//!
//! Regression guard for the `parquet-write-read` E2E scenario, which failed
//! on the Pi 5 cluster (2026-04-18) with HTTP 500 and the body
//!
//! ```json
//! {"error":"SQL query failed: Telemetry query failed:
//!          SQL queries not supported by this telemetry backend"}
//! ```
//!
//! TC-350 already guards DataFusion SQL over `ParquetTelemetryStore`, but the
//! live binary at `src/main.rs` was wiring up `JsonlTelemetryStore`, which
//! does not override the default `query_sql` trait method. The TC-350 green
//! status therefore did not reflect what the real server did on the cluster.
//!
//! **Invariant under test:** whatever `TelemetryStore` implementation the
//! composition root (`src/main.rs`) instantiates MUST answer
//! `/api/telemetry/query` with HTTP 200 for
//! `SELECT COUNT(*) FROM metrics` after ingestion and for
//! `SELECT COUNT(*) FROM traces` with no data — never 500 with
//! "SQL queries not supported by this telemetry backend".
//!
//! This test imports `picloud_http::build_main_telemetry_store` — the very
//! same factory `src/main.rs` now uses to build its telemetry store. If that
//! factory ever regresses to a backend without SQL support this test fails
//! with the exact same error the E2E scenario saw on the Pis.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use picloud_domain::iri::ClusterDomain;
use picloud_events::InMemoryEventLog;
use picloud_http::{build_main_telemetry_store, OtelStream, PiCloudHttpServer};
use picloud_rdf::OxigraphProjector;

// ---------------------------------------------------------------------------
// Test infrastructure — single-node server built via the exact same factory
// the composition root (`src/main.rs`) uses. The Invariant is that whichever
// backend comes out of that factory MUST support SQL.
// ---------------------------------------------------------------------------

fn test_server_with_main_factory() -> (axum::Router, std::path::PathBuf) {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let otel_stream = Arc::new(OtelStream::new(4096));
    let telemetry_dir = std::env::temp_dir().join(format!(
        "picloud-tc356-{}",
        uuid::Uuid::new_v4()
    ));

    // Use the same factory main.rs uses — mirrors `src/main.rs:1019-1022`.
    // If the composition root's backend ever loses SQL support, this test
    // will fail with the exact error surfaced by the cluster E2E.
    let telemetry_store_concrete =
        build_main_telemetry_store(&telemetry_dir, 168 /* default 7 days */);
    let telemetry_store: Arc<dyn picloud_domain::traits::TelemetryStore> =
        telemetry_store_concrete.clone();

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
// Fakes — minimal trait impls (mirrors tc350_telemetry_sql_query_endpoint_returns_200.rs)
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
// TC-356 — Main-binary telemetry backend must support SQL queries
// ===========================================================================

#[tokio::test]
async fn tc356_main_binary_telemetry_backend_supports_sql() {
    let (app, telemetry_dir) = test_server_with_main_factory();

    // -----------------------------------------------------------------
    // Step 1: SELECT COUNT(*) FROM traces on an empty store.
    //
    // This is the exact query the Pi 5 E2E scenario used and which
    // returned HTTP 500 with:
    //   "SQL queries not supported by this telemetry backend"
    //
    // The invariant is HTTP 200 regardless of whether traces exist.
    // -----------------------------------------------------------------
    let traces_query = serde_json::json!({
        "signal": "traces",
        "sql":    "SELECT COUNT(*) FROM traces",
    });

    let (status, raw, body) = post_json(&app, "/api/telemetry/query", traces_query).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Main-binary telemetry backend must answer SQL with 200 even on an \
         empty traces table. Got {status}. Body: {raw}"
    );
    assert!(
        body.get("error").is_none(),
        "Response body must not contain an error field. Body: {raw}"
    );
    // Specifically guard against the exact "not supported" error that broke
    // the Pi 5 cluster.
    assert!(
        !raw.contains("SQL queries not supported by this telemetry backend"),
        "Backend reported that SQL queries are not supported — the \
         composition root is wiring up a telemetry backend without SQL \
         support. Body: {raw}"
    );

    // -----------------------------------------------------------------
    // Step 2: Ingest a small OTel metric batch via /otel.
    // -----------------------------------------------------------------
    let metrics_payload = serde_json::json!({
        "metrics": [
            {
                "name": "http_request_duration_ms",
                "value": 42.5,
                "unit": "ms",
                "metric_type": "gauge",
                "service_name": "tc356-test-service",
                "timestamp": "2026-04-18T13:00:00Z",
                "attributes": {"method": "GET"}
            },
            {
                "name": "http_requests_total",
                "value": 1024.0,
                "unit": "count",
                "metric_type": "counter",
                "service_name": "tc356-test-service",
                "timestamp": "2026-04-18T13:00:01Z",
                "attributes": {"method": "GET"}
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
        Some(2),
        "Should accept 2 metric points. Body: {raw}"
    );

    // Allow the direct-write path to flush.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // -----------------------------------------------------------------
    // Step 3: SELECT COUNT(*) AS total FROM metrics — the invariant
    //         spelled out in the TC front-matter.
    // -----------------------------------------------------------------
    let metrics_query = serde_json::json!({
        "signal": "metrics",
        "sql":    "SELECT COUNT(*) AS total FROM metrics",
    });

    let (status, raw, body) = post_json(&app, "/api/telemetry/query", metrics_query).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "Telemetry SQL query must return 200 after ingestion. Got {status}. \
         Body: {raw}"
    );
    assert!(
        body.get("error").is_none(),
        "Response must not contain an error field. Body: {raw}"
    );
    assert!(
        !raw.contains("SQL queries not supported by this telemetry backend"),
        "Main-binary backend does not support SQL — wire-up regression. \
         Body: {raw}"
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

    // Aggregate must reflect the 2 metric points we just ingested.
    let total = rows[0]
        .get("total")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    assert_eq!(
        total, 2,
        "COUNT(*) must equal the 2 ingested metric points, got {total}. \
         Body: {raw}"
    );

    // -----------------------------------------------------------------
    // Step 4: Repeat the exact query the Pi 5 E2E scenario ran — now on
    //         a store that has metrics but still no traces. Must stay
    //         at HTTP 200.
    // -----------------------------------------------------------------
    let traces_query = serde_json::json!({
        "signal": "traces",
        "sql":    "SELECT COUNT(*) FROM traces",
    });
    let (status, raw, body) = post_json(&app, "/api/telemetry/query", traces_query).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "SELECT COUNT(*) FROM traces must return 200 even when there are \
         no traces. Got {status}. Body: {raw}"
    );
    assert!(
        body.get("error").is_none(),
        "Response must not contain an error field. Body: {raw}"
    );

    // Cleanup so CI does not accumulate state.
    let _ = std::fs::remove_dir_all(&telemetry_dir);
}
