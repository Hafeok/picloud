//! ADR-043: Config live reload — update a config value and verify the new value is reflected.
//!
//! Update a config value, then GET it. Assert the new value is returned.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ConfigLiveReload;

#[async_trait]
impl Scenario for ConfigLiveReload {
    fn name(&self) -> &str {
        "config-live-reload"
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

        if !assertions::feature_available(ctx, config_path).await {
            return ScenarioResult::Skip {
                reason: "config API not implemented yet".to_string(),
            };
        }

        // Create an initial config entry.
        let initial = serde_json::json!({
            "key": "test.reload.key",
            "value": "initial-value",
            "type": "string"
        });

        let post_resp = match assertions::http_post(ctx, config_path, initial).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST initial config failed: {}", e),
                };
            }
        };

        if !post_resp.status().is_success() && post_resp.status().as_u16() != 409 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("POST initial config returned status {}", post_resp.status()),
            };
        }

        // Update the config entry with a new value.
        let updated = serde_json::json!({
            "key": "test.reload.key",
            "value": "updated-value",
            "type": "string"
        });

        let put_resp = match assertions::http_post(ctx, config_path, updated).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST updated config failed: {}", e),
                };
            }
        };

        if !put_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("POST updated config returned status {}", put_resp.status()),
            };
        }

        // Brief pause for projection to update.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // GET the config entry and verify it reflects the updated value.
        let get_resp = match assertions::http_get(
            ctx,
            &format!("{}/test.reload.key", config_path),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("GET updated config failed: {}", e),
                };
            }
        };

        if !get_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("GET updated config returned status {}", get_resp.status()),
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

        if !body.contains("updated-value") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "config entry still shows old value after update — live reload not working: {}",
                    body
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
