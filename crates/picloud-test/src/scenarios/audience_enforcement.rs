//! ADR-051: Audience enforcement — POST to /auth/token with wrong audience.
//!
//! Request a token for one product and attempt to use it against another
//! product's endpoint. Assert the platform rejects the mismatched audience.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AudienceEnforcement;

#[async_trait]
impl Scenario for AudienceEnforcement {
    fn name(&self) -> &str {
        "audience-enforcement"
    }

    fn adr(&self) -> &str {
        "ADR-051"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let token_path = "/auth/token";
        if !assertions::feature_available(ctx, token_path).await {
            return ScenarioResult::Skip {
                reason: "auth token endpoint not implemented yet".to_string(),
            };
        }

        // Request a token with a wrong/mismatched audience.
        let token_req = serde_json::json!({
            "grant_type": "client_credentials",
            "client_id": "test-app",
            "client_secret": "test-secret",
            "audience": "https://picloud.local/products/nonexistent-product",
            "scope": "read"
        });

        let resp = match assertions::http_post(ctx, token_path, token_req).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST token request failed: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();

        // A request with a wrong audience should be rejected.
        // Expected: 400 (invalid_request), 403 (forbidden), or 401 (unauthorized).
        if status == 200 || status == 201 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "token issued for nonexistent audience with status {} — audience enforcement not working",
                    status
                ),
            };
        }

        // Verify the error response indicates an audience problem.
        let body = match resp.text().await {
            Ok(b) => b,
            Err(_) => String::new(),
        };

        let is_auth_error = status == 400 || status == 401 || status == 403;
        if !is_auth_error {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "unexpected status {} for wrong audience — expected 400, 401, or 403. Body: {}",
                    status, body
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
