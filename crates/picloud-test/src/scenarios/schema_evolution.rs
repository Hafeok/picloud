//! ADR-031: Schema evolution — apply event with schema v1, then v2. Assert both
//! coexist.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SchemaEvolution;

#[async_trait]
impl Scenario for SchemaEvolution {
    fn name(&self) -> &str {
        "schema-evolution"
    }

    fn adr(&self) -> &str {
        "ADR-031"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/events").await {
            return ScenarioResult::Skip {
                reason: "event API not available".to_string(),
            };
        }

        // Emit an event with schema v1.
        let v1_event = serde_json::json!({
            "type": "SchemaTestEvent",
            "schema": "https://picloud.local/schemas/events/SchemaTestEvent/v1",
            "source": "picloud-test",
            "payload": {
                "name": "test-resource",
                "version": 1
            }
        });

        match assertions::http_post(ctx, "/api/events", v1_event).await {
            Ok(resp) if resp.status().is_success() => {
                info!("v1 schema event emitted");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!("v1 event emission returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("v1 event emission failed: {}", e),
                };
            }
        }

        // Emit an event with schema v2.
        let v2_event = serde_json::json!({
            "type": "SchemaTestEvent",
            "schema": "https://picloud.local/schemas/events/SchemaTestEvent/v2",
            "source": "picloud-test",
            "payload": {
                "name": "test-resource",
                "version": 2,
                "additionalField": "new-in-v2"
            }
        });

        match assertions::http_post(ctx, "/api/events", v2_event).await {
            Ok(resp) if resp.status().is_success() => {
                info!("v2 schema event emitted");
            }
            Ok(resp) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("v2 event emission returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("v2 event emission failed: {}", e),
                };
            }
        }

        // Wait for projection.
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify both schema versions coexist in the RDF graph.
        let coexist_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(DISTINCT ?schema) AS ?count) WHERE {
                ?event a picloud:Event ;
                       picloud:schema ?schema .
                FILTER(CONTAINS(STR(?schema), "SchemaTestEvent"))
            }
        "#;

        match assertions::sparql_query(ctx, coexist_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse schema count: {}", e),
                        };
                    }
                };

                let schema_count: u64 = json
                    .pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                if schema_count >= 2 {
                    info!(
                        schemas = schema_count,
                        "multiple schema versions coexist"
                    );
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expected >= 2 distinct schema versions, found {}",
                            schema_count
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("schema coexistence query failed: {}", e),
            },
        }
    }
}
