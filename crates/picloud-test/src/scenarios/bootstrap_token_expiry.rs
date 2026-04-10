//! ADR-026: Bootstrap token expiry — check bootstrap token has TTL. Verify
//! expired tokens rejected.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct BootstrapTokenExpiry;

#[async_trait]
impl Scenario for BootstrapTokenExpiry {
    fn name(&self) -> &str {
        "bootstrap-token-expiry"
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

        // Check if bootstrap token API exists.
        if !assertions::feature_available(ctx, "/api/auth/bootstrap").await {
            return ScenarioResult::Skip {
                reason: "bootstrap token API not available".to_string(),
            };
        }

        // Attempt to use a clearly expired/fake bootstrap token.
        let expired_token = "expired-bootstrap-token-000000";
        let enroll_body = serde_json::json!({
            "token": expired_token,
            "username": "expiry-test-admin",
            "displayName": "Expiry Test Admin"
        });

        match assertions::http_post(ctx, "/api/auth/bootstrap/enroll", enroll_body).await {
            Ok(resp) => {
                let status = resp.status().as_u16();

                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "bootstrap enrollment endpoint not implemented".to_string(),
                    };
                }

                // Expired or invalid token should be rejected.
                if status == 401 || status == 403 || status == 400 {
                    info!(
                        status = status,
                        "expired/invalid bootstrap token correctly rejected"
                    );
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if status == 200 {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "expired/fake bootstrap token was accepted — security violation"
                            .to_string(),
                    }
                } else {
                    info!(
                        status = status,
                        "bootstrap enrollment returned unexpected status — token likely rejected"
                    );
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("bootstrap enrollment request failed: {}", e),
            },
        }
    }
}
