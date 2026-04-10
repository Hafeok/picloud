//! ADR-041: Trigger same alert condition rapidly. Assert dampening prevents
//! flood (minimum 60-second hold-off before re-firing same alert).

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AlertDampening;

#[async_trait]
impl Scenario for AlertDampening {
    fn name(&self) -> &str {
        "alert-dampening"
    }

    fn adr(&self) -> &str {
        "ADR-041"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Fire an alert by injecting high CPU temp
        let fire_cmd = serde_json::json!({
            "type": "MetricRecorded",
            "source": "https://picloud.local/nodes/dampening-test-node",
            "payload": {
                "node_iri": "https://picloud.local/nodes/dampening-test-node",
                "metrics": [
                    { "name": "cpu_temp_celsius", "value": 85.0, "unit": "celsius" }
                ]
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", fire_cmd.clone()).await {
            return ScenarioResult::Skip {
                reason: format!("metric injection not available: {}", e),
            };
        }

        // 2. Wait briefly for alert to fire
        tokio::time::sleep(Duration::from_secs(5)).await;

        // 3. Clear the condition
        let clear_cmd = serde_json::json!({
            "type": "MetricRecorded",
            "source": "https://picloud.local/nodes/dampening-test-node",
            "payload": {
                "node_iri": "https://picloud.local/nodes/dampening-test-node",
                "metrics": [
                    { "name": "cpu_temp_celsius", "value": 60.0, "unit": "celsius" }
                ]
            }
        });

        let _ = assertions::http_post(ctx, "/api/commands", clear_cmd).await;
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 4. Re-fire within 60 seconds — should be dampened
        let _ = assertions::http_post(ctx, "/api/commands", fire_cmd).await;
        tokio::time::sleep(Duration::from_secs(3)).await;

        // 5. Count AlertFired events for this node — should be at most 1
        let count_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?alert) AS ?count) WHERE {
                ?alert a picloud:AlertFired ;
                       picloud:alertResource <https://picloud.local/nodes/dampening-test-node> ;
                       picloud:alertType "HighCpuTemperature" .
            }
        "#;

        match assertions::sparql_query(ctx, count_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let count_str = json
                    .pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0");
                let count: u64 = count_str.parse().unwrap_or(0);

                if count <= 1 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "dampening failed: {} AlertFired events within 60s window (expected <= 1)",
                            count
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Skip {
                reason: format!("alert query not available: {}", e),
            },
        }
    }
}
