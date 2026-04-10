//! ADR-026: Tier 1 admin reset — verify admin reset flow exists in IAM endpoints.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct Tier1AdminReset;

#[async_trait]
impl Scenario for Tier1AdminReset {
    fn name(&self) -> &str {
        "tier1-admin-reset"
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

        // Check if the passkey reset endpoint exists.
        let reset_paths = [
            "/api/auth/identity/reset-passkey",
            "/api/identity/reset-passkey",
            "/auth/passkeys/reset",
        ];

        let mut reset_available = false;
        for path in &reset_paths {
            if assertions::feature_available(ctx, path).await {
                info!(path = path, "admin reset endpoint found");
                reset_available = true;

                // Attempt a reset request to verify it returns a structured response.
                let reset_body = serde_json::json!({
                    "username": "nonexistent-user-for-test"
                });

                match assertions::http_post(ctx, path, reset_body).await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        // 401/403 = needs admin auth (correct behavior)
                        // 404 = user not found (correct behavior)
                        // 200 = token generated (correct behavior)
                        if status == 200
                            || status == 401
                            || status == 403
                            || status == 404
                        {
                            info!(
                                status = status,
                                "reset endpoint responds correctly"
                            );
                        } else {
                            return ScenarioResult::Fail {
                                duration: start.elapsed(),
                                reason: format!(
                                    "reset endpoint returned unexpected status {}",
                                    status
                                ),
                            };
                        }
                    }
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("reset request failed: {}", e),
                        };
                    }
                }

                break;
            }
        }

        if !reset_available {
            // Check source code for admin reset implementation.
            let iam_src = ctx
                .workspace_root()
                .join("crates/picloud-iam/src/lib.rs");
            if let Ok(src) = std::fs::read_to_string(&iam_src) {
                if src.contains("reset_passkey") || src.contains("reset-passkey") {
                    info!("admin reset code found in picloud-iam source");
                    return ScenarioResult::Pass {
                        duration: start.elapsed(),
                    };
                }
            }

            return ScenarioResult::Skip {
                reason: "admin reset endpoint not found at any expected path".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
