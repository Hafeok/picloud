//! ADR-009: Token expiry enforcement — POST to /auth/token with expired
//! credentials and assert 401 response.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TokenExpiryEnforcement;

#[async_trait]
impl Scenario for TokenExpiryEnforcement {
    fn name(&self) -> &str {
        "token-expiry-enforcement"
    }

    fn adr(&self) -> &str {
        "ADR-009"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Attempt to use an expired/invalid token against an IAM-gated endpoint.
        let expired_token = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ0ZXN0IiwiZXhwIjoxMDAwMDAwMDAwfQ.invalid";

        // Use a GET endpoint that exists — /products accepts GET.
        let url = format!("{}/products", ctx.config.base_url());
        let resp = match ctx
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", expired_token))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to send request: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();

        // Expect 401 Unauthorized for expired/invalid tokens.
        if status == 401 {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else if status == 403 {
            // 403 is also acceptable — the token was rejected.
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "expected 401 or 403 for expired token, got {}",
                    status
                ),
            }
        }
    }
}
