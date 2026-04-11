//! ADR-053: Check enrollment token has TTL. Verify expired tokens are rejected.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TokenEnrollExpiry;

#[async_trait]
impl Scenario for TokenEnrollExpiry {
    fn name(&self) -> &str {
        "token-enroll-expiry"
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

        // 2. Generate an enrollment token with a short TTL
        let token_req = serde_json::json!({
            "ttl_seconds": 5,
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

        // 3. Verify token has expiry field
        let has_expiry = json.get("expires_at").is_some() || json.get("ttl").is_some();
        if !has_expiry {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "enrollment token response missing TTL/expiry field".to_string(),
            };
        }

        // 4. Wait for token to expire
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        // 5. Attempt enrollment with expired token
        let expired_token = json
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("expired-test-token");

        let enroll_body = serde_json::json!({
            "csr": "test-csr-data",
            "token": expired_token,
        });

        match assertions::http_post(ctx, "/auth/enroll", enroll_body).await {
            Ok(resp) => {
                let enroll_status = resp.status().as_u16();
                if enroll_status == 400 || enroll_status == 401 || enroll_status == 403 || enroll_status == 410 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expired enrollment token was not rejected (status {})",
                            enroll_status
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST enrollment with expired token: {}", e),
            },
        }
    }
}
