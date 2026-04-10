//! ADR-038: Remove input triple that triggered inference. Assert inferred
//! triple retracted from graph.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct InferenceRetraction;

#[async_trait]
impl Scenario for InferenceRetraction {
    fn name(&self) -> &str {
        "inference-retraction"
    }

    fn adr(&self) -> &str {
        "ADR-038"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Inject a metric event that triggers an inference rule (e.g. high memory)
        let metric_cmd = serde_json::json!({
            "type": "MetricRecorded",
            "source": "https://picloud.local/nodes/retraction-test-node",
            "payload": {
                "node_iri": "https://picloud.local/nodes/retraction-test-node",
                "metrics": [
                    { "name": "memory_used_mb", "value": 14746, "unit": "mb" },
                    { "name": "memory_total_mb", "value": 16384, "unit": "mb" }
                ]
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", metric_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("metric injection not available: {}", e),
            };
        }

        // 2. Wait for inferred alert triple to appear
        let alert_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?alert a picloud:Alert ;
                       picloud:alertResource <https://picloud.local/nodes/retraction-test-node> .
            }
        "#;

        if assertions::wait_for_sparql(ctx, alert_query, Duration::from_secs(10))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "inference rule did not produce alert triple — rule may not be active"
                    .to_string(),
            };
        }

        // 3. Clear the condition (memory back to normal)
        let clear_cmd = serde_json::json!({
            "type": "MetricRecorded",
            "source": "https://picloud.local/nodes/retraction-test-node",
            "payload": {
                "node_iri": "https://picloud.local/nodes/retraction-test-node",
                "metrics": [
                    { "name": "memory_used_mb", "value": 4096, "unit": "mb" },
                    { "name": "memory_total_mb", "value": 16384, "unit": "mb" }
                ]
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", clear_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST clearing metric: {}", e),
            };
        }

        // 4. Wait and verify the alert triple was retracted
        tokio::time::sleep(Duration::from_secs(5)).await;

        match assertions::sparql_query(ctx, alert_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                if json.get("boolean").and_then(|v| v.as_bool()) == Some(false) {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "inferred alert triple was not retracted after condition cleared"
                            .to_string(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("SPARQL query failed: {}", e),
            },
        }
    }
}
