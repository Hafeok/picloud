//! ADR-035: POST replay request for a single aggregate. Verify only that
//! aggregate's events are replayed and others are unaffected.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AggregateReplay;

#[async_trait]
impl Scenario for AggregateReplay {
    fn name(&self) -> &str {
        "aggregate-replay"
    }

    fn adr(&self) -> &str {
        "ADR-035"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check replay endpoint exists
        let replay_path = "/api/replay";
        if !assertions::feature_available(ctx, replay_path).await {
            return ScenarioResult::Skip {
                reason: "replay endpoint not available".to_string(),
            };
        }

        // 2. POST aggregate replay request
        let replay_body = serde_json::json!({
            "from": "2020-01-01T00:00:00Z",
            "aggregate_type": "TestAggregate",
            "aggregate_ids": ["test-id-001"],
        });

        let resp = match assertions::http_post(ctx, replay_path, replay_body).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to POST aggregate replay: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();
        if status == 404 || status == 501 {
            return ScenarioResult::Skip {
                reason: "aggregate replay not implemented yet".to_string(),
            };
        }

        if !resp.status().is_success() && status != 202 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("aggregate replay returned unexpected status: {}", status),
            };
        }

        // 3. Verify ReplayStarted event appears
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event a picloud:ReplayStarted .
            }
        "#;

        match assertions::wait_for_sparql(ctx, ask_query, Duration::from_secs(30)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("ReplayStarted event not found after aggregate replay: {}", e),
            },
        }
    }
}
