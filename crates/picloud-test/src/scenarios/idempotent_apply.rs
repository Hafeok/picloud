//! ADR-015: Idempotent apply — apply same resource twice, assert second produces
//! zero new events.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct IdempotentApply;

#[async_trait]
impl Scenario for IdempotentApply {
    fn name(&self) -> &str {
        "idempotent-apply"
    }

    fn adr(&self) -> &str {
        "ADR-015"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/apply").await {
            return ScenarioResult::Skip {
                reason: "resource API not available".to_string(),
            };
        }

        // Get current event count.
        let count_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?e) AS ?count) WHERE {
                ?e a picloud:Event .
            }
        "#;

        let resource_json = serde_json::json!({
            "type": "container",
            "name": "idempotent-test",
            "product": "picloud-test",
            "idempotencyKey": "idempotent-test-key-001",
            "spec": {
                "image": "alpine:latest",
                "command": ["sleep", "3600"]
            }
        });

        let first_apply_body = serde_json::json!({
            "resources": [resource_json]
        });

        // First apply.
        match assertions::http_post(ctx, "/api/apply", first_apply_body.clone()).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!("first apply completed");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!("resource apply returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("resource apply failed: {}", e),
                };
            }
        }

        // Get event count after first apply.
        let count_after_first = match assertions::sparql_query(ctx, count_query).await {
            Ok(b) => {
                let json: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                json.pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query event count: {}", e),
                };
            }
        };

        info!(count = count_after_first, "event count after first apply");

        // Second apply with same idempotency key.
        match assertions::http_post(ctx, "/api/apply", first_apply_body).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!("second apply completed");
            }
            Ok(resp) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "second apply returned unexpected status {}",
                        resp.status()
                    ),
                };
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("second apply failed: {}", e),
                };
            }
        }

        // Get event count after second apply — should be unchanged.
        let count_after_second = match assertions::sparql_query(ctx, count_query).await {
            Ok(b) => {
                let json: serde_json::Value = serde_json::from_str(&b).unwrap_or_default();
                json.pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query event count after second apply: {}", e),
                };
            }
        };

        info!(count = count_after_second, "event count after second apply");

        if count_after_second != count_after_first {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "second apply produced new events: {} before, {} after",
                    count_after_first, count_after_second
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
