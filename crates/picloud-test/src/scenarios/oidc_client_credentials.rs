//! ADR-017: OIDC client credentials — POST /auth/token with client_credentials
//! grant. Assert token or structured error.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OidcClientCredentials;

#[async_trait]
impl Scenario for OidcClientCredentials {
    fn name(&self) -> &str {
        "oidc-client-credentials"
    }

    fn adr(&self) -> &str {
        "ADR-017"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Check if the token endpoint exists.
        if !assertions::feature_available(ctx, "/auth/token").await {
            return ScenarioResult::Skip {
                reason: "token endpoint not available".to_string(),
            };
        }

        // POST client credentials grant.
        let token_url = format!("{}/auth/token", ctx.config.base_url());
        let resp = match ctx
            .http_client
            .post(&token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials&client_id=test-client&client_secret=test-secret")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("token request failed: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read token response: {}", e),
                };
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "token response is not JSON (status {}): {}",
                        status, e
                    ),
                };
            }
        };

        if status == 200 {
            // Successful token issuance — verify required fields.
            let has_access_token = json.get("access_token").is_some();
            let token_type = json
                .get("token_type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let has_expires_in = json.get("expires_in").is_some();

            if !has_access_token {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "token response missing access_token".to_string(),
                };
            }

            if !token_type.eq_ignore_ascii_case("bearer") {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "token_type should be 'Bearer', got '{}'",
                        token_type
                    ),
                };
            }

            if !has_expires_in {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "token response missing expires_in".to_string(),
                };
            }

            info!("client credentials token issued successfully");
        } else if status == 401 || status == 400 {
            // Expected error for test credentials — verify structured error response.
            let has_error = json.get("error").is_some();
            if !has_error {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "token endpoint returned {} but no 'error' field in response",
                        status
                    ),
                };
            }

            info!(
                status = status,
                error = %json.get("error").unwrap(),
                "token endpoint correctly rejected test credentials"
            );
        } else {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "token endpoint returned unexpected status {}: {}",
                    status, body
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
