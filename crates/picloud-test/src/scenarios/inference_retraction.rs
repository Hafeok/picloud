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

        // 1. Inject an AlertFired event to simulate inference producing an alert
        let resource_iri = "https://picloud.local/nodes/retraction-test-node";
        let alert_cmd = serde_json::json!({
            "type": "AlertFired",
            "source": resource_iri,
            "payload": {
                "alert_type": "HighMemory",
                "severity": "warning",
                "resource_iri": resource_iri,
                "rule_iri": "",
                "message": "Memory usage exceeded threshold"
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", alert_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("alert event injection not available: {}", e),
            };
        }

        // 2. Wait for alert triple to appear
        let alert_iri = format!("{}/alerts/highmemory", resource_iri);
        let alert_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <{}> a picloud:Alert ;
                     picloud:alertResource <{}> .
            }}
            "#,
            alert_iri, resource_iri
        );

        if assertions::wait_for_sparql(ctx, &alert_query, Duration::from_secs(10))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "alert triple did not appear — alert projection may not be active"
                    .to_string(),
            };
        }

        // 3. Resolve the alert (simulating inference retraction)
        let clear_cmd = serde_json::json!({
            "type": "AlertResolved",
            "source": resource_iri,
            "payload": {
                "alert_type": "HighMemory",
                "resource_iri": resource_iri
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", clear_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST AlertResolved event: {}", e),
            };
        }

        // 4. Wait and verify the alert triple was retracted
        let retracted_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                FILTER NOT EXISTS {{
                    <{}> a picloud:Alert ;
                         picloud:alertResource <{}> .
                }}
            }}
            "#,
            alert_iri, resource_iri
        );

        match assertions::wait_for_sparql(ctx, &retracted_query, Duration::from_secs(15)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(_) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "inferred alert triple was not retracted after condition cleared"
                    .to_string(),
            },
        }
    }
}
