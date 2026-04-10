//! ADR-035: POST full platform replay. Assert graph rebuilt from event log.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PlatformReplayFull;

#[async_trait]
impl Scenario for PlatformReplayFull {
    fn name(&self) -> &str {
        "platform-replay-full"
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

        // 1. Check cluster replay endpoint
        if !assertions::feature_available(ctx, "/api/cluster/replay").await {
            return ScenarioResult::Skip {
                reason: "cluster replay endpoint not available".to_string(),
            };
        }

        // 2. Record current triple count before replay
        let count_query = "SELECT (COUNT(*) AS ?count) WHERE { ?s ?p ?o }";
        let pre_count = match assertions::sparql_query(ctx, count_query).await {
            Ok(body) => body,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query pre-replay triple count: {}", e),
                };
            }
        };

        // 3. POST full platform replay
        let replay_body = serde_json::json!({
            "from": "1970-01-01T00:00:00Z",
        });

        let resp = match assertions::http_post(ctx, "/api/cluster/replay", replay_body).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to POST platform replay: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();
        if status == 404 || status == 501 {
            return ScenarioResult::Skip {
                reason: "platform replay not implemented yet".to_string(),
            };
        }

        if !resp.status().is_success() && status != 202 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("platform replay returned unexpected status: {}", status),
            };
        }

        // 4. Wait for ReplayCompleted event
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event a picloud:ReplayCompleted .
            }
        "#;

        match assertions::wait_for_sparql(ctx, ask_query, Duration::from_secs(60)).await {
            Ok(()) => {}
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("ReplayCompleted event not found: {}", e),
                };
            }
        }

        // 5. Verify triple count is unchanged after replay (graph rebuilt correctly)
        let post_count = match assertions::sparql_query(ctx, count_query).await {
            Ok(body) => body,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query post-replay triple count: {}", e),
                };
            }
        };

        if pre_count != post_count {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "triple count changed after replay: pre={}, post={}",
                    pre_count, post_count
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
