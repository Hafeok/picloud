//! ADR-013: Full replication coverage — write on one node, read from another.
//! Assert data present on all nodes.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct FullReplicationCoverage;

#[async_trait]
impl Scenario for FullReplicationCoverage {
    fn name(&self) -> &str {
        "full-replication-coverage"
    }

    fn adr(&self) -> &str {
        "ADR-013"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if ctx.config.nodes.len() < 2 {
            return ScenarioResult::Skip {
                reason: "need at least 2 nodes for replication test".to_string(),
            };
        }

        // Write a sentinel event via the primary endpoint.
        let sentinel_id = format!("repl-test-{}", uuid::Uuid::new_v4());
        let event_body = serde_json::json!({
            "type": "TestSentinel",
            "source": "picloud-test",
            "payload": {
                "sentinel_id": sentinel_id
            }
        });

        match assertions::http_post(ctx, "/api/events", event_body).await {
            Ok(resp) if resp.status().is_success() => {
                info!(sentinel = %sentinel_id, "sentinel event written");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!("event API returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("event API unavailable: {}", e),
                };
            }
        }

        // Query each node directly to verify the sentinel is present.
        for node in &ctx.config.nodes {
            let node_url = format!(
                "https://{}:{}/sparql",
                node.ip, ctx.config.cluster.http_port
            );

            let query = format!(
                r#"
                PREFIX picloud: <https://picloud.local/ontology#>
                ASK {{
                    ?event picloud:sentinelId "{}" .
                }}
                "#,
                sentinel_id
            );

            let resp = match ctx
                .http_client
                .post(&node_url)
                .header("Content-Type", "application/sparql-query")
                .header("Accept", "application/sparql-results+json")
                .body(query)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "failed to query node {}: {}",
                            node.hostname, e
                        ),
                    };
                }
            };

            if !resp.status().is_success() {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "SPARQL query on node {} returned status {}",
                        node.hostname,
                        resp.status()
                    ),
                };
            }

            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "failed to read response from node {}: {}",
                            node.hostname, e
                        ),
                    };
                }
            };

            let json: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "failed to parse SPARQL response from node {}: {}",
                            node.hostname, e
                        ),
                    };
                }
            };

            if json.get("boolean").and_then(|v| v.as_bool()) != Some(true) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "sentinel {} not found on node {}",
                        sentinel_id, node.hostname
                    ),
                };
            }

            info!(node = %node.hostname, "sentinel verified on node");
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
