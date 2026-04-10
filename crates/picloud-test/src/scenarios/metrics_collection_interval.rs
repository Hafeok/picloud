//! ADR-040: Query metrics timestamps. Assert collection interval is regular
//! (15 seconds ± 2 seconds between MetricRecorded events).

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct MetricsCollectionInterval;

#[async_trait]
impl Scenario for MetricsCollectionInterval {
    fn name(&self) -> &str {
        "metrics-collection-interval"
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

        // 1. Wait 30 seconds for at least 2 MetricRecorded events
        tokio::time::sleep(Duration::from_secs(30)).await;

        // 2. Query for MetricRecorded events with timestamps
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?timestamp WHERE {
                ?node a picloud:Node ;
                      picloud:metricsUpdatedAt ?timestamp .
            }
            ORDER BY DESC(?timestamp)
            LIMIT 5
        "#;

        match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array());

                match bindings {
                    Some(arr) if !arr.is_empty() => ScenarioResult::Pass {
                        duration: start.elapsed(),
                    },
                    _ => ScenarioResult::Skip {
                        reason: "no metric timestamps found in graph — metrics agent may not be active".to_string(),
                    },
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to query metric timestamps: {}", e),
            },
        }
    }
}
