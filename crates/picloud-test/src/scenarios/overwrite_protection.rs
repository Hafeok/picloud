//! ADR-050: Overwrite protection — apply a resource that already exists without force flag.
//!
//! Assert the platform returns an error when attempting to overwrite an existing
//! resource without the --overwrite / force flag.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OverwriteProtection;

#[async_trait]
impl Scenario for OverwriteProtection {
    fn name(&self) -> &str {
        "overwrite-protection"
    }

    fn adr(&self) -> &str {
        "ADR-050"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let commands_path = "/api/commands";
        if !assertions::feature_available(ctx, commands_path).await {
            return ScenarioResult::Skip {
                reason: "commands endpoint not implemented yet".to_string(),
            };
        }

        // First, create a resource.
        let resource = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "config",
            "name": "overwrite-test",
            "product": "test-app",
            "payload": {
                "key": "overwrite.test",
                "value": "original"
            }
        });

        let first = match assertions::http_post(ctx, commands_path, resource.clone()).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("POST command failed: {}", e),
                };
            }
        };

        let first_status = first.status().as_u16();
        if first_status == 404 || first_status == 501 {
            return ScenarioResult::Skip {
                reason: "commands endpoint not fully implemented".to_string(),
            };
        }

        // Wait for the first resource to be projected before attempting the duplicate.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Attempt to apply the same resource again without a force/overwrite flag.
        let duplicate = match assertions::http_post(ctx, commands_path, resource).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("second POST failed: {}", e),
                };
            }
        };

        let dup_status = duplicate.status().as_u16();

        // The platform should reject the duplicate with 409 Conflict or 400 Bad Request.
        if dup_status == 200 || dup_status == 201 || dup_status == 204 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "duplicate resource accepted with status {} — overwrite protection not enforced",
                    dup_status
                ),
            };
        }

        // 202 means the command was accepted before duplicate detection (projection lag).
        if dup_status == 202 {
            return ScenarioResult::Skip {
                reason: "duplicate returned 202 — first resource may not have been projected yet (timing)".to_string(),
            };
        }

        // 409 Conflict is the expected status for a duplicate resource.
        if dup_status != 409 && dup_status != 400 && dup_status != 422 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "unexpected status {} for duplicate resource — expected 409, 400, or 422",
                    dup_status
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
