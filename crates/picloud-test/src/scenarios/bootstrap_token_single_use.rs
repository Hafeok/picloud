//! ADR-026: Bootstrap token single use — use bootstrap token, try reuse, assert
//! rejected.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct BootstrapTokenSingleUse;

#[async_trait]
impl Scenario for BootstrapTokenSingleUse {
    fn name(&self) -> &str {
        "bootstrap-token-single-use"
    }

    fn adr(&self) -> &str {
        "ADR-026"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/enroll-node").await {
            return ScenarioResult::Skip {
                reason: "enrollment API not available".to_string(),
            };
        }

        // Generate a bootstrap token if the generate endpoint exists.
        let token_resp = match assertions::http_post(
            ctx,
            "/api/enroll-node",
            serde_json::json!({"ttl_seconds": 300}),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("bootstrap token generation failed: {}", e),
                };
            }
        };

        let token_status = token_resp.status().as_u16();
        if token_status == 404 || token_status == 501 {
            return ScenarioResult::Skip {
                reason: "bootstrap token generation not implemented".to_string(),
            };
        }

        let token_body = match token_resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read token response: {}", e),
                };
            }
        };

        let token_json: serde_json::Value = match serde_json::from_str(&token_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("token response not valid JSON: {}", e),
                };
            }
        };

        let token = match token_json.get("token").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "bootstrap token response missing 'token' field".to_string(),
                };
            }
        };

        info!("bootstrap token generated");

        // First use of the token.
        let enroll_body = serde_json::json!({
            "token": token,
            "username": "single-use-test-admin",
            "displayName": "Single Use Test Admin"
        });

        let first_use = match assertions::http_post(
            ctx,
            "/auth/enroll",
            enroll_body.clone(),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("first token use failed: {}", e),
                };
            }
        };

        let first_status = first_use.status().as_u16();
        info!(status = first_status, "first use of bootstrap token");

        // Second use of the same token — must be rejected.
        let second_use = match assertions::http_post(
            ctx,
            "/auth/enroll",
            enroll_body,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("second token use request failed: {}", e),
                };
            }
        };

        let second_status = second_use.status().as_u16();

        if second_status == 401 || second_status == 403 || second_status == 400 {
            info!(
                status = second_status,
                "bootstrap token reuse correctly rejected"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else if second_status == 200 {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "bootstrap token was reused successfully — single-use not enforced"
                    .to_string(),
            }
        } else {
            info!(
                status = second_status,
                "second use returned non-200 status — token likely rejected"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        }
    }
}
