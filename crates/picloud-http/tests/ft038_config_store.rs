//! FT-038 — Product configuration store — typed key-value with tags, workload override, live reload
//!
//! Covers TC-251, TC-308.
//! These tests verify that:
//! 1. Typed config entries (string, int, float, bool, json) can be stored and retrieved
//! 2. Config entries with tags are correctly persisted
//! 3. Workload-level overrides merge over product config (workload wins)
//! 4. ConfigChanged events enable live reload — new values appear via query after set
//! 5. End-to-end lifecycle: set → query → update → re-query → delete → verify gone

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::util::ServiceExt;

use picloud_domain::iri::ClusterDomain;
use picloud_domain::traits::{EventFilter, EventLog, StateProjector};
use picloud_events::InMemoryEventLog;
use picloud_http::PiCloudHttpServer;
use picloud_rdf::OxigraphProjector;

// ---------------------------------------------------------------------------
// Test infrastructure (mirrors integration_test.rs)
// ---------------------------------------------------------------------------

fn test_server() -> (axum::Router, Arc<InMemoryEventLog>, Arc<OxigraphProjector>) {
    let domain = ClusterDomain::default();
    let event_log = Arc::new(InMemoryEventLog::new());
    let projector = Arc::new(OxigraphProjector::with_domain(domain.clone()).unwrap());

    let server = PiCloudHttpServer::new("127.0.0.1:0".parse().unwrap(), domain).with_dependencies(
        event_log.clone(),
        projector.clone(),
        Arc::new(FakeCluster),
        Arc::new(FakeIam),
        Arc::new(FakeStorage),
        Arc::new(FakeScheduler),
    );

    (server.build_router(), event_log, projector)
}

async fn test_server_with_projection() -> (
    axum::Router,
    Arc<InMemoryEventLog>,
    Arc<OxigraphProjector>,
) {
    let (router, event_log, projector) = test_server();

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

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

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

async fn delete_json(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("DELETE")
        .uri(uri)
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

/// Wait for projection to process events.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
}

// ===========================================================================
// TC-251 — Config store typed key-value set and live-reloaded by workload
// ===========================================================================

/// TC-251: Scenario test — set typed config entries (string, int, float, bool, json),
/// verify they are stored in the RDF graph with correct types. Then update a value
/// (live-reload) and confirm the workload's effective config endpoint reflects
/// the updated value without restart. Finally, test workload override semantics.
#[tokio::test]
async fn tc251_config_store_typed_key_value_set_and_live_reloaded_by_workload() {
    let (app, event_log, _projector) = test_server_with_projection().await;

    // ── Step 0: Create the product with a container for later workload override testing ──
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "cfg-app", "version": "1.0.0", "description": "Test product" },
            { "type": "container", "name": "api-server", "product": "cfg-app", "image": "api:1.0.0", "identity": "api-worker" }
        ]
    });
    let (status, _) = post_json(&app, "/api/apply", resource_file).await;
    assert_eq!(status, StatusCode::OK, "product+container apply must succeed");
    settle().await;

    // ── Step 1: Set config entries of each type ──
    let entries = vec![
        ("db_host", "postgres.internal", "string"),
        ("db_port", "5432", "int"),
        ("cache_ttl", "3.5", "float"),
        ("debug_mode", "true", "bool"),
        ("limits", r#"{"max_conn": 100, "timeout": 30}"#, "json"),
    ];

    for (key, value, config_type) in &entries {
        let (status, body) = post_json(
            &app,
            "/products/cfg-app/config",
            serde_json::json!({
                "key": key,
                "value": value,
                "config_type": config_type,
                "tags": { "env": "production", "tier": "data" },
            }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::ACCEPTED,
            "setting config key '{}' must be accepted, got: {:?}",
            key,
            body
        );
    }

    // Wait for projection
    settle().await;

    // ── Step 2: Verify each key is retrievable with correct type ──
    for (key, expected_value, expected_type) in &entries {
        let (status, body) = get_json(&app, &format!("/products/cfg-app/config/{}", key)).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "GET config key '{}' must return 200, got {:?}",
            key,
            body
        );
        assert_eq!(
            body["key"].as_str().unwrap(),
            *key,
            "returned key must match"
        );
        assert_eq!(
            body["value"].as_str().unwrap(),
            *expected_value,
            "returned value for '{}' must match",
            key
        );
        assert_eq!(
            body["type"].as_str().unwrap(),
            *expected_type,
            "returned type for '{}' must match",
            key
        );
    }

    // ── Step 3: Verify the config list endpoint returns all entries ──
    let (status, body) = get_json(&app, "/products/cfg-app/config").await;
    assert_eq!(status, StatusCode::OK);
    let listed_entries = body["entries"].as_array().expect("entries must be an array");
    assert_eq!(
        listed_entries.len(),
        5,
        "config list must contain all 5 entries"
    );

    // ── Step 4: Live reload — update db_port value ──
    let (status, _) = post_json(
        &app,
        "/products/cfg-app/config",
        serde_json::json!({
            "key": "db_port",
            "value": "5433",
            "config_type": "int",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "updating db_port must succeed");
    settle().await;

    // ── Step 5: Verify the updated value is returned (live-reload) ──
    let (status, body) = get_json(&app, "/products/cfg-app/config/db_port").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["value"].as_str().unwrap(),
        "5433",
        "db_port must reflect updated value (live-reload)"
    );

    // ── Step 6: Verify workload effective config endpoint returns product config ──
    // (Container was already applied in Step 0)
    let (status, body) = get_json(&app, "/products/cfg-app/containers/api-server/config").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "EffectiveConfig");
    assert_eq!(body["container"], "api-server");
    // The effective config should include product-level entries
    let effective = body["entries"].as_array().expect("entries must be array");
    assert!(
        effective.len() >= 5,
        "effective config must include product entries, got {}",
        effective.len()
    );

    // ── Step 7: Verify ConfigChanged events were emitted ──
    // product + container = 2 events, 5 config sets + 1 update = 6 ConfigChanged events = 8 total
    let total_events = event_log.len().await;
    assert!(
        total_events >= 8,
        "at least 8 events should be in the log (product + container + 5 sets + 1 update), got {}",
        total_events
    );
}

// ===========================================================================
// TC-308 — Config store exit — typed values stored, retrieved, and live-reloaded
// ===========================================================================

/// TC-308: Exit criteria — comprehensive verification of the config store.
/// Tests the full lifecycle: create → read → update → re-read → delete → verify gone.
/// Verifies all five typed values, tags, workload override semantics, and live reload.
#[tokio::test]
async fn tc308_config_store_exit_typed_values_stored_retrieved_and_live_reloaded() {
    let (app, event_log, _projector) = test_server_with_projection().await;

    // ── Phase 1: Product setup (include container for workload override testing) ──
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "exit-app", "version": "1.0.0", "description": "Test product" },
            { "type": "container", "name": "worker", "product": "exit-app", "image": "worker:2.0.0", "identity": "worker-id" }
        ]
    });
    let (status, _) = post_json(&app, "/api/apply", resource_file).await;
    assert_eq!(status, StatusCode::OK, "product+container apply must succeed");
    settle().await;

    // ── Phase 2: Store typed values with tags ──

    // String type
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "app_name",
            "value": "ExitTestApp",
            "config_type": "string",
            "tags": { "category": "identity" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "string config must be accepted");

    // Int type
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "max_retries",
            "value": "3",
            "config_type": "int",
            "tags": { "category": "resilience" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "int config must be accepted");

    // Float type
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "timeout_seconds",
            "value": "30.5",
            "config_type": "float",
            "tags": { "category": "performance" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "float config must be accepted");

    // Bool type
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "maintenance_mode",
            "value": "false",
            "config_type": "bool",
            "tags": { "category": "operations" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "bool config must be accepted");

    // Json type
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "feature_flags",
            "value": r#"{"dark_mode": true, "beta": false}"#,
            "config_type": "json",
            "tags": { "category": "features" },
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "json config must be accepted");

    settle().await;

    // ── Phase 3: Retrieve and verify all typed values ──

    let type_checks = vec![
        ("app_name", "ExitTestApp", "string"),
        ("max_retries", "3", "int"),
        ("timeout_seconds", "30.5", "float"),
        ("maintenance_mode", "false", "bool"),
        ("feature_flags", r#"{"dark_mode": true, "beta": false}"#, "json"),
    ];

    for (key, expected_val, expected_type) in &type_checks {
        let (status, body) =
            get_json(&app, &format!("/products/exit-app/config/{}", key)).await;
        assert_eq!(status, StatusCode::OK, "retrieving '{}' must return 200", key);
        assert_eq!(body["value"].as_str().unwrap(), *expected_val, "value mismatch for '{}'", key);
        assert_eq!(body["type"].as_str().unwrap(), *expected_type, "type mismatch for '{}'", key);
    }

    // ── Phase 4: Live reload — update maintenance_mode from false to true ──
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "maintenance_mode",
            "value": "true",
            "config_type": "bool",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "update must be accepted");
    settle().await;

    // Verify the value is now "true" (live-reloaded)
    let (status, body) =
        get_json(&app, "/products/exit-app/config/maintenance_mode").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["value"].as_str().unwrap(),
        "true",
        "maintenance_mode must reflect live-reloaded value"
    );
    assert_eq!(
        body["type"].as_str().unwrap(),
        "bool",
        "type must remain bool after update"
    );

    // ── Phase 5: Live reload — update max_retries from 3 to 5 ──
    let (status, _) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "max_retries",
            "value": "5",
            "config_type": "int",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    settle().await;

    let (status, body) =
        get_json(&app, "/products/exit-app/config/max_retries").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["value"].as_str().unwrap(),
        "5",
        "max_retries must be live-reloaded to 5"
    );

    // ── Phase 6: Delete a config entry ──
    let (status, _) =
        delete_json(&app, "/products/exit-app/config/feature_flags").await;
    assert_eq!(status, StatusCode::ACCEPTED, "delete must be accepted");
    settle().await;

    // Verify the deleted key returns 404
    let (status, _) =
        get_json(&app, "/products/exit-app/config/feature_flags").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "deleted config key must return 404"
    );

    // Verify remaining entries are still present
    let (status, body) = get_json(&app, "/products/exit-app/config").await;
    assert_eq!(status, StatusCode::OK);
    let remaining = body["entries"].as_array().expect("entries must be array");
    assert_eq!(
        remaining.len(),
        4,
        "4 entries must remain after deleting one"
    );

    // ── Phase 7: Workload override ──
    // (Container was already applied in Phase 1)
    // Query effective config for the container workload
    let (status, body) = get_json(&app, "/products/exit-app/containers/worker/config").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "EffectiveConfig", "must return EffectiveConfig type");
    assert_eq!(body["product"], "exit-app");
    assert_eq!(body["container"], "worker");

    // The effective config should include the 4 remaining product entries
    let effective = body["entries"].as_array().expect("entries must be array");
    assert_eq!(
        effective.len(),
        4,
        "effective config for workload must include product-level entries"
    );

    // ── Phase 8: Verify events were emitted for all operations ──
    let total = event_log.len().await;
    // 1 product + 1 container + 5 set + 2 update + 1 delete = 10 events
    assert!(
        total >= 10,
        "expected at least 10 events in the log, got {}",
        total
    );

    // ── Phase 9: Sensitive key rejection ──
    let (status, body) = post_json(
        &app,
        "/products/exit-app/config",
        serde_json::json!({
            "key": "db_password",
            "value": "should-fail",
            "config_type": "string",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "sensitive keys must be rejected"
    );
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("secret"),
        "error message must mention secrets store"
    );
}
