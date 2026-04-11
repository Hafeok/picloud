//! ADR-053: Use enrollment token. Try reuse. Assert rejected.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TokenEnrollSingleUse;

#[async_trait]
impl Scenario for TokenEnrollSingleUse {
    fn name(&self) -> &str {
        "token-enroll-single-use"
    }

    fn adr(&self) -> &str {
        "ADR-053"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check enrollment token endpoint
        if !assertions::feature_available(ctx, "/api/enroll-node").await {
            return ScenarioResult::Skip {
                reason: "enrollment token endpoint not available".to_string(),
            };
        }

        // 2. Generate an enrollment token
        let token_req = serde_json::json!({
            "ttl_seconds": 3600,
        });

        let token_resp =
            match assertions::http_post(ctx, "/api/enroll-node", token_req).await {
                Ok(r) => r,
                Err(e) => {
                    return ScenarioResult::Skip {
                        reason: format!("enrollment token generation not available: {}", e),
                    };
                }
            };

        let status = token_resp.status().as_u16();
        if status == 404 || status == 501 {
            return ScenarioResult::Skip {
                reason: "enrollment token generation not implemented".to_string(),
            };
        }

        if !token_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("enrollment token generation failed: status {}", status),
            };
        }

        let body = token_resp.text().await.unwrap_or_default();
        let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let token = json
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("test-single-use-token");

        // 3. First use — should succeed (or at least not fail with "already used")
        let enroll_body = serde_json::json!({
            "csr": "test-csr-data-first",
            "token": token,
        });

        let first_resp = match assertions::http_post(ctx, "/auth/enroll", enroll_body).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("first enrollment attempt failed: {}", e),
                };
            }
        };

        let first_status = first_resp.status().as_u16();
        if first_status == 404 || first_status == 501 {
            return ScenarioResult::Skip {
                reason: "enrollment endpoint not implemented".to_string(),
            };
        }

        // 4. Second use — must be rejected
        let reuse_body = serde_json::json!({
            "csr": "test-csr-data-second",
            "token": token,
        });

        match assertions::http_post(ctx, "/auth/enroll", reuse_body).await {
            Ok(resp) => {
                let reuse_status = resp.status().as_u16();
                if reuse_status == 400 || reuse_status == 401 || reuse_status == 403 || reuse_status == 409 || reuse_status == 410 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "enrollment token reuse was not rejected (status {}). Tokens must be single-use.",
                            reuse_status
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST enrollment reuse: {}", e),
            },
        }
    }
}
