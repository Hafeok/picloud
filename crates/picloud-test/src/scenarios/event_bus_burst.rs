//! ADR-018: Event bus burst — post many events rapidly. Assert all are processed.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventBusBurst;

#[async_trait]
impl Scenario for EventBusBurst {
    fn name(&self) -> &str {
        "event-bus-burst"
    }

    fn adr(&self) -> &str {
        "ADR-018"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/commands").await {
            return ScenarioResult::Skip {
                reason: "event bus API not available".to_string(),
            };
        }

        let burst_id = format!("burst-{}", uuid::Uuid::new_v4());
        let event_count = 100;

        // Post events in rapid succession using ResourceDeclared so the
        // projector actually creates triples for each one.
        let mut success_count = 0u32;
        for i in 0..event_count {
            let resource_name = format!("burst-res-{}-{}", burst_id, i);
            let resource_iri = format!(
                "https://picloud.local/products/burst-test/resources/{}",
                resource_name
            );
            let event_body = serde_json::json!({
                "type": "ResourceDeclared",
                "source": resource_iri,
                "product": "burst-test",
                "payload": {
                    "resource_iri": resource_iri,
                    "resource_type": "BurstResource",
                    "product": "burst-test",
                    "burstId": burst_id,
                    "index": i
                }
            });

            match assertions::http_post(ctx, "/api/commands", event_body).await {
                Ok(resp) if resp.status().is_success() => {
                    success_count += 1;
                }
                Ok(resp) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "event {} returned status {}",
                            i,
                            resp.status()
                        ),
                    };
                }
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("event {} failed: {}", i, e),
                    };
                }
            }
        }

        info!(sent = success_count, "burst events posted");

        // Wait briefly for projection to catch up.
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Verify all events are projected into the RDF graph.
        // Each ResourceDeclared event creates a triple with resourceType "BurstResource"
        // scoped to product "burst-test". Count resources whose IRI contains the burst_id.
        let count_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?e) AS ?count) WHERE {{
                ?e picloud:resourceType "BurstResource" .
                FILTER(CONTAINS(STR(?e), "{}"))
            }}
            "#,
            burst_id
        );

        match assertions::sparql_query(ctx, &count_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse count response: {}", e),
                        };
                    }
                };

                let projected_count: u32 = json
                    .pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                if projected_count < event_count {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "only {}/{} events projected — {} lost",
                            projected_count,
                            event_count,
                            event_count - projected_count
                        ),
                    };
                }

                info!(
                    projected = projected_count,
                    expected = event_count,
                    "all burst events projected"
                );
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("burst count query failed: {}", e),
            },
        }
    }
}
