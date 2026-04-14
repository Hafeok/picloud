//! FT-039 — Feature flags — version-bound on/off, SDK evaluation, event invalidation
//!
//! Covers TC-252, TC-309.
//! These tests verify that:
//! 1. Feature flags can be created via /api/apply with version expressions
//! 2. Flag evaluation reflects the product version (version-bound on/off)
//! 3. Toggling a flag via POST /products/:name/flags changes its state
//! 4. SDK evaluation (GET /products/:name/flags/:flag) reflects the new state
//! 5. FeatureFlagChanged events enable live invalidation — new values appear after toggle
//! 6. End-to-end lifecycle: create → evaluate → toggle → re-evaluate → delete → verify gone

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
// TC-252 — Feature flag toggled and SDK evaluation reflects new state
// ===========================================================================

/// TC-252: Scenario test — create a product at version 2.1.0, declare feature flags
/// with various version expressions, then verify SDK evaluation (GET endpoint) returns
/// correct active/inactive state. Toggle a flag (disable it) and verify the evaluation
/// reflects the new state without restart.
#[tokio::test]
async fn tc252_feature_flag_toggled_and_sdk_evaluation_reflects_new_state() {
    let (app, event_log, _projector) = test_server_with_projection().await;

    // ── Step 0: Create the product at version 2.1.0 with feature flags in a single apply ──
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "flag-app", "version": "2.1.0", "description": "Test product for feature flags" },
            {
                "type": "feature-flag",
                "name": "new-upload-flow",
                "product": "flag-app",
                "description": "Enables the redesigned upload flow",
                "enabled": true,
                "version": "= 2"
            },
            {
                "type": "feature-flag",
                "name": "dark-mode",
                "product": "flag-app",
                "description": "Dark mode UI",
                "enabled": true,
                "version": ">= 2"
            },
            {
                "type": "feature-flag",
                "name": "legacy-api",
                "product": "flag-app",
                "description": "Legacy v1 API compatibility shim",
                "enabled": true,
                "version": "< 2"
            },
            {
                "type": "feature-flag",
                "name": "beta-search",
                "product": "flag-app",
                "description": "Experimental search",
                "enabled": true,
                "version": "2..4"
            }
        ]
    });
    let (status, body) = post_json(&app, "/api/apply", resource_file).await;
    assert_eq!(status, StatusCode::OK, "product+flags apply must succeed: {:?}", body);
    settle().await;

    // ── Step 2: Verify SDK evaluation — GET individual flags ──

    // new-upload-flow: version "= 2", product at 2.1.0 → major 2 → active
    let (status, body) = get_json(&app, "/products/flag-app/flags/new-upload-flow").await;
    assert_eq!(status, StatusCode::OK, "flag GET must return 200");
    assert_eq!(body["name"], "new-upload-flow");
    assert_eq!(body["active"], true, "new-upload-flow should be active for version 2.1.0 (= 2)");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["version"], "= 2");

    // dark-mode: version ">= 2", product at 2.1.0 → major 2 → active
    let (status, body) = get_json(&app, "/products/flag-app/flags/dark-mode").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "dark-mode should be active for version 2.1.0 (>= 2)");

    // legacy-api: version "< 2", product at 2.1.0 → major 2 → inactive
    let (status, body) = get_json(&app, "/products/flag-app/flags/legacy-api").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false, "legacy-api should be inactive for version 2.1.0 (< 2)");

    // beta-search: version "2..4", product at 2.1.0 → major 2 → active
    let (status, body) = get_json(&app, "/products/flag-app/flags/beta-search").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "beta-search should be active for version 2.1.0 (2..4)");

    // ── Step 3: Verify the flags list endpoint returns all flags ──
    let (status, body) = get_json(&app, "/products/flag-app/flags").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["productVersion"], "2.1.0");
    let flags = body["flags"].as_array().expect("flags must be an array");
    assert_eq!(flags.len(), 4, "flags list must contain all 4 flags");

    // ── Step 4: Toggle a flag — disable new-upload-flow via POST ──
    let (status, body) = post_json(
        &app,
        "/products/flag-app/flags",
        serde_json::json!({
            "name": "new-upload-flow",
            "enabled": false,
            "version": "= 2",
            "description": "Enables the redesigned upload flow",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "flag toggle must be accepted, got: {:?}", body);
    settle().await;

    // ── Step 5: Verify SDK evaluation reflects new state (disabled) ──
    let (status, body) = get_json(&app, "/products/flag-app/flags/new-upload-flow").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false, "new-upload-flow should be inactive after disabling");
    assert_eq!(body["enabled"], false, "enabled should be false after toggle");

    // ── Step 6: Re-enable the flag ──
    let (status, _) = post_json(
        &app,
        "/products/flag-app/flags",
        serde_json::json!({
            "name": "new-upload-flow",
            "enabled": true,
            "version": "= 2",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    settle().await;

    let (status, body) = get_json(&app, "/products/flag-app/flags/new-upload-flow").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "new-upload-flow should be active again after re-enabling");

    // ── Step 7: Verify FeatureFlagChanged events were emitted ──
    // 1 product + 4 flags via apply + 2 toggles (disable + enable) = 7 events minimum
    let total_events = event_log.len().await;
    assert!(
        total_events >= 7,
        "at least 7 events should be in the log (1 product + 4 flags + 2 toggles), got {}",
        total_events
    );
}

// ===========================================================================
// TC-309 — Feature flags exit — flag toggled and SDK evaluation updated
// ===========================================================================

/// TC-309: Exit criteria — comprehensive verification of feature flags.
/// Tests the full lifecycle: create → evaluate → toggle → re-evaluate → update version
/// expression → re-evaluate → delete → verify gone.
/// Verifies version-bound evaluation for all six operators, live toggle, and event invalidation.
#[tokio::test]
async fn tc309_feature_flags_exit_flag_toggled_and_sdk_evaluation_updated() {
    let (app, event_log, _projector) = test_server_with_projection().await;

    // ── Phase 1: Product setup at version 3.0.0 ──
    let resource_file = serde_json::json!({
        "resources": [
            { "type": "product", "name": "exit-flags", "version": "3.0.0", "description": "Exit criteria product" }
        ]
    });
    let (status, _) = post_json(&app, "/api/apply", resource_file).await;
    assert_eq!(status, StatusCode::OK, "product apply must succeed");
    settle().await;

    // ── Phase 2: Create flags covering all six version operators ──
    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "exact-v3",
            "enabled": true,
            "version": "= 3",
            "description": "Exact match v3",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "gt-v2",
            "enabled": true,
            "version": "> 2",
            "description": "Greater than v2",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "gte-v3",
            "enabled": true,
            "version": ">= 3",
            "description": "Greater than or equal v3",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "lt-v5",
            "enabled": true,
            "version": "< 5",
            "description": "Less than v5",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "lte-v3",
            "enabled": true,
            "version": "<= 3",
            "description": "Less than or equal v3",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "range-2-4",
            "enabled": true,
            "version": "2..4",
            "description": "Range 2 to 4 inclusive",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    settle().await;

    // ── Phase 3: Verify all flags evaluate correctly for product version 3.0.0 (major=3) ──

    // exact-v3: "= 3" matches major 3 → active
    let (status, body) = get_json(&app, "/products/exit-flags/flags/exact-v3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "exact-v3 should be active for version 3.0.0");
    assert_eq!(body["enabled"], true);

    // gt-v2: "> 2" matches major 3 → active
    let (status, body) = get_json(&app, "/products/exit-flags/flags/gt-v2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "gt-v2 should be active for version 3.0.0");

    // gte-v3: ">= 3" matches major 3 → active
    let (status, body) = get_json(&app, "/products/exit-flags/flags/gte-v3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "gte-v3 should be active for version 3.0.0");

    // lt-v5: "< 5" matches major 3 → active
    let (status, body) = get_json(&app, "/products/exit-flags/flags/lt-v5").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "lt-v5 should be active for version 3.0.0");

    // lte-v3: "<= 3" matches major 3 → active
    let (status, body) = get_json(&app, "/products/exit-flags/flags/lte-v3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "lte-v3 should be active for version 3.0.0");

    // range-2-4: "2..4" matches major 3 → active
    let (status, body) = get_json(&app, "/products/exit-flags/flags/range-2-4").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "range-2-4 should be active for version 3.0.0");

    // ── Phase 4: Verify flags list endpoint ──
    let (status, body) = get_json(&app, "/products/exit-flags/flags").await;
    assert_eq!(status, StatusCode::OK);
    let flags = body["flags"].as_array().expect("flags must be an array");
    assert_eq!(flags.len(), 6, "flags list must contain all 6 flags");
    assert_eq!(body["productVersion"], "3.0.0");

    // ── Phase 5: Toggle — disable exact-v3 ──
    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "exact-v3",
            "enabled": false,
            "version": "= 3",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "toggle must be accepted");
    settle().await;

    // Verify flag is now inactive (disabled even though version matches)
    let (status, body) = get_json(&app, "/products/exit-flags/flags/exact-v3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false, "exact-v3 should be inactive after disabling");
    assert_eq!(body["enabled"], false);

    // ── Phase 6: Update version expression — change range-2-4 to "= 5" (should become inactive) ──
    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "range-2-4",
            "enabled": true,
            "version": "= 5",
            "description": "Changed to exact v5",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    settle().await;

    let (status, body) = get_json(&app, "/products/exit-flags/flags/range-2-4").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], false, "range-2-4 should be inactive after changing to version = 5");
    assert_eq!(body["version"], "= 5", "version expression should be updated");

    // ── Phase 7: Re-enable exact-v3 ──
    let (status, _) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "exact-v3",
            "enabled": true,
            "version": "= 3",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    settle().await;

    let (status, body) = get_json(&app, "/products/exit-flags/flags/exact-v3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["active"], true, "exact-v3 should be active again after re-enabling");

    // ── Phase 8: Delete a flag ──
    let (status, _) = delete_json(&app, "/products/exit-flags/flags/gt-v2").await;
    assert_eq!(status, StatusCode::ACCEPTED, "delete must be accepted");
    settle().await;

    // Verify deleted flag returns 404
    let (status, _) = get_json(&app, "/products/exit-flags/flags/gt-v2").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "deleted flag must return 404");

    // Verify remaining flags still exist
    let (status, body) = get_json(&app, "/products/exit-flags/flags").await;
    assert_eq!(status, StatusCode::OK);
    let remaining = body["flags"].as_array().expect("flags must be an array");
    assert_eq!(remaining.len(), 5, "5 flags must remain after deleting one");

    // ── Phase 9: Invalid version expression is rejected ──
    let (status, body) = post_json(
        &app,
        "/products/exit-flags/flags",
        serde_json::json!({
            "name": "bad-flag",
            "enabled": true,
            "version": "~= 3",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid version expression must be rejected");
    assert!(
        body["error"].as_str().unwrap().contains("invalid version expression"),
        "error message must mention invalid version expression"
    );

    // ── Phase 10: Verify events were emitted for all operations ──
    let total = event_log.len().await;
    // 1 product + 6 flag creates + 3 toggles/updates + 1 delete = 11 events minimum
    assert!(
        total >= 11,
        "expected at least 11 events in the log, got {}",
        total
    );

    // ── Phase 11: Non-existent flag returns 404 ──
    let (status, _) = get_json(&app, "/products/exit-flags/flags/nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent flag must return 404");
}
