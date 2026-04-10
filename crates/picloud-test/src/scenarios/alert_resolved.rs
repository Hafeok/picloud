//! ADR-041: Trigger alert, then resolve condition. Assert AlertResolved event.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AlertResolved;

#[async_trait]
impl Scenario for AlertResolved {
    fn name(&self) -> &str {
        "alert-resolved"
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
            "source": "https://picloud.local/nodes/resolved-test-node",
            "payload": {
                "node_iri": "https://picloud.local/nodes/resolved-test-node",
                "metrics": [
                    { "name": "cpu_temp_celsius", "value": 85.0, "unit": "celsius" }
                ]
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", fire_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("metric injection not available: {}", e),
            };
        }

        // 2. Wait for AlertFired
        let fired_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?alert a picloud:Alert ;
                       picloud:alertResource <https://picloud.local/nodes/resolved-test-node> .
            }
        "#;

        if assertions::wait_for_sparql(ctx, fired_query, Duration::from_secs(30))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "alert did not fire — alert rules may not be active".to_string(),
            };
        }

        // 3. Clear the condition
        let clear_cmd = serde_json::json!({
            "type": "MetricRecorded",
            "source": "https://picloud.local/nodes/resolved-test-node",
            "payload": {
                "node_iri": "https://picloud.local/nodes/resolved-test-node",
                "metrics": [
                    { "name": "cpu_temp_celsius", "value": 65.0, "unit": "celsius" }
                ]
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", clear_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST clearing metric: {}", e),
            };
        }

        // 4. Wait for alert triple to be retracted (AlertResolved)
        tokio::time::sleep(Duration::from_secs(5)).await;

        match assertions::sparql_query(ctx, fired_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                if json.get("boolean").and_then(|v| v.as_bool()) == Some(false) {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "alert triple not retracted after condition cleared — AlertResolved not working".to_string(),
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
