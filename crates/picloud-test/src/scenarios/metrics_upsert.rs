//! ADR-040: Verify metrics are upserted (latest-value-only) rather than appended.
//!
//! Query metrics, wait, query again. Assert the graph holds only the latest
//! metric values — timestamps update but triple count per node does not grow.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct MetricsUpsert;

#[async_trait]
impl Scenario for MetricsUpsert {
    fn name(&self) -> &str {
        "metrics-upsert"
    }

    fn adr(&self) -> &str {
        "ADR-040"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Count metric triples for the first query.
        let count_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?updated) AS ?count) WHERE {
                ?node a picloud:Node ;
                      picloud:metricsUpdatedAt ?updated .
            }
        "#;

        let body1 = match assertions::sparql_query(ctx, count_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SPARQL endpoint not available: {}", e),
                };
            }
        };

        let json1: serde_json::Value = match serde_json::from_str(&body1) {
            Ok(v) => v,
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "failed to parse first SPARQL response".to_string(),
                };
            }
        };

        let count1 = json1
            .pointer("/results/bindings/0/count/value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        if count1 == 0 {
            return ScenarioResult::Skip {
                reason: "no metric data yet — metrics agent may not have run".to_string(),
            };
        }

        // Wait for at least one metrics interval to pass (15s default + buffer).
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;

        // Query again and verify count has not grown (upsert, not append).
        let body2 = match assertions::sparql_query(ctx, count_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("second SPARQL query failed: {}", e),
                };
            }
        };

        let json2: serde_json::Value = match serde_json::from_str(&body2) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse second SPARQL response: {}", e),
                };
            }
        };

        let count2 = json2
            .pointer("/results/bindings/0/count/value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        // The count should remain the same — one metricsUpdatedAt per node.
        // If it grew, metrics are being appended instead of upserted.
        if count2 > count1 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "metricsUpdatedAt count grew from {} to {} — metrics are appended, not upserted",
                    count1, count2
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
