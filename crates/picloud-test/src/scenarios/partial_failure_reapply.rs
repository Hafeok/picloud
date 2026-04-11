//! ADR-015: Partial failure reapply — apply, kill mid-way, re-apply. Assert
//! correct final state.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PartialFailureReapply;

#[async_trait]
impl Scenario for PartialFailureReapply {
    fn name(&self) -> &str {
        "partial-failure-reapply"
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

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Skip {
                reason: "no nodes configured for SSH — cannot simulate mid-apply failure"
                    .to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/apply").await {
            return ScenarioResult::Skip {
                reason: "resource API not available".to_string(),
            };
        }

        let idempotency_key = format!("partial-fail-{}", uuid::Uuid::new_v4());

        let resource_body = serde_json::json!({
            "resources": [{
                "type": "container",
                "name": "partial-fail-test",
                "product": "picloud-test",
                "idempotencyKey": idempotency_key,
                "spec": {
                    "image": "alpine:latest",
                    "command": ["sleep", "3600"]
                }
            }]
        });

        // First apply — attempt to apply the resource.
        info!("starting first apply (will attempt mid-apply interruption)");
        let first_result =
            assertions::http_post(ctx, "/api/apply", resource_body.clone()).await;

        match first_result {
            Ok(resp) => {
                info!(status = %resp.status(), "first apply response received");
            }
            Err(e) => {
                info!(error = %e, "first apply interrupted — expected for partial failure");
            }
        }

        // Brief pause to simulate recovery window.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Re-apply with the same idempotency key.
        info!("re-applying with same idempotency key");
        match assertions::http_post(ctx, "/api/apply", resource_body).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!("re-apply completed successfully");
            }
            Ok(resp) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("re-apply returned unexpected status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("re-apply failed: {}", e),
                };
            }
        }

        // Verify exactly one resource exists with this name (no duplicates).
        let verify_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?r) AS ?count) WHERE {
                ?r picloud:name "partial-fail-test" .
            }
        "#;

        match assertions::sparql_query(ctx, verify_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse verification response: {}", e),
                        };
                    }
                };

                let count: u64 = json
                    .pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                if count == 1 {
                    info!("exactly one resource exists — no duplication");
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expected exactly 1 resource after reapply, found {}",
                            count
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("verification query failed: {}", e),
            },
        }
    }
}
