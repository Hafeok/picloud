//! ADR-035: Mark events as replayed. Verify replay flag set on re-emitted events.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ReplayMarkedFlag;

#[async_trait]
impl Scenario for ReplayMarkedFlag {
    fn name(&self) -> &str {
        "replay-marked-flag"
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

        // 1. Check replay endpoint
        if !assertions::feature_available(ctx, "/api/replay").await {
            return ScenarioResult::Skip {
                reason: "replay endpoint not available".to_string(),
            };
        }

        // 2. Trigger a replay
        let replay_body = serde_json::json!({
            "from": "1970-01-01T00:00:00Z",
        });

        let resp = match assertions::http_post(ctx, "/api/replay", replay_body).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to POST replay: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();
        if status == 404 || status == 501 {
            return ScenarioResult::Skip {
                reason: "replay not implemented yet".to_string(),
            };
        }

        // 3. Wait for replay to complete and check for replay-marked events
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event picloud:isReplay true ;
                       picloud:replayId ?replayId .
            }
        "#;

        match assertions::wait_for_sparql(ctx, ask_query, Duration::from_secs(30)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "no replay-marked events found — replayed events must carry is_replay: true and replay_id: {}",
                    e
                ),
            },
        }
    }
}
