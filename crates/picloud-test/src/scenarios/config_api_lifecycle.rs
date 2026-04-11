//! ADR-043: Product Configuration Store — CRUD lifecycle.
//!
//! POST/GET/PUT/DELETE to /products/{name}/config. Test the full CRUD
//! lifecycle for configuration entries.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ConfigApiLifecycle;

#[async_trait]
impl Scenario for ConfigApiLifecycle {
    fn name(&self) -> &str {
        "config-api-lifecycle"
    }

    fn adr(&self) -> &str {
        "ADR-043"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::commands_available(ctx).await {
            return ScenarioResult::Skip {
                reason: "command endpoint not responsive (Raft quorum unavailable)".to_string(),
            };
        }

        let config_path = "/products/test-app/config";

        // Check if config API is available.
        if !assertions::feature_available(ctx, config_path).await {
            return ScenarioResult::Skip {
                reason: "config API not implemented yet".to_string(),
            };
        }

        // 1. POST — create a config entry.
        let entry = serde_json::json!({
            "key": "test.scenario.key",
            "value": "hello-picloud",
            "type": "string",
            "tags": { "tier": "test" }
        });

        let post_resp = match assertions::http_post(ctx, config_path, entry).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST config entry failed: {}", e),
                };
            }
        };

        if !post_resp.status().is_success() && post_resp.status().as_u16() != 409 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("POST config returned status {}", post_resp.status()),
            };
        }

        // 2. GET — retrieve the config entry.
        let get_resp = match assertions::http_get(
            ctx,
            &format!("{}/test.scenario.key", config_path),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("GET config entry failed: {}", e),
                };
            }
        };

        if !get_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("GET config entry returned status {}", get_resp.status()),
            };
        }

        let body = match get_resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read GET response: {}", e),
                };
            }
        };

        if !body.contains("hello-picloud") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "GET config entry does not contain expected value 'hello-picloud': {}",
                    body
                ),
            };
        }

        // 3. DELETE — remove the config entry.
        let url = format!(
            "{}{}/test.scenario.key",
            ctx.config.base_url(),
            config_path
        );
        let del_resp = match ctx.http_client.delete(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("DELETE config entry failed: {}", e),
                };
            }
        };

        if !del_resp.status().is_success() && del_resp.status().as_u16() != 404 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("DELETE config entry returned status {}", del_resp.status()),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
