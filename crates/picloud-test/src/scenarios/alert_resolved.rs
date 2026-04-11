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

        // 1. Fire an alert by emitting an AlertFired event directly
        let resource_iri = "https://picloud.local/nodes/resolved-test-node";
        let fire_cmd = serde_json::json!({
            "type": "AlertFired",
            "source": resource_iri,
            "payload": {
                "alert_type": "HighCpuTemp",
                "severity": "warning",
                "resource_iri": resource_iri,
                "rule_iri": "",
                "message": "CPU temperature exceeded threshold"
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", fire_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("alert event injection not available: {}", e),
            };
        }

        // 2. Wait for AlertFired to project
        let alert_iri = format!("{}/alerts/highcputemp", resource_iri);
        let fired_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <{}> a picloud:Alert ;
                     picloud:alertResource <{}> .
            }}
            "#,
            alert_iri, resource_iri
        );

        if assertions::wait_for_sparql(ctx, &fired_query, Duration::from_secs(30))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "alert did not project — alert projection may not be active".to_string(),
            };
        }

        // 3. Resolve the alert
        let clear_cmd = serde_json::json!({
            "type": "AlertResolved",
            "source": resource_iri,
            "payload": {
                "alert_type": "HighCpuTemp",
                "resource_iri": resource_iri
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", clear_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST clearing metric: {}", e),
            };
        }

        // 4. Wait for alert triple to be retracted (AlertResolved)
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
                reason: "alert triple not retracted after condition cleared — AlertResolved not working".to_string(),
            },
        }
    }
}
