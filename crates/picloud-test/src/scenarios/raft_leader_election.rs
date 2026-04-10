//! ADR-002: Raft leader election — assert exactly one leader node in the cluster.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RaftLeaderElection;

#[async_trait]
impl Scenario for RaftLeaderElection {
    fn name(&self) -> &str {
        "raft-leader-election"
    }

    fn adr(&self) -> &str {
        "ADR-002"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query for leader nodes — there must be exactly one.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?node WHERE {
                ?node picloud:hasRole picloud:Leader .
            }
        "#;

        match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse SPARQL response: {}", e),
                        };
                    }
                };

                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array());

                match bindings {
                    Some(arr) if arr.len() == 1 => ScenarioResult::Pass {
                        duration: start.elapsed(),
                    },
                    Some(arr) => ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expected exactly 1 leader node, found {}",
                            arr.len()
                        ),
                    },
                    None => ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "SPARQL response missing /results/bindings".to_string(),
                    },
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("SPARQL query failed: {}", e),
            },
        }
    }
}
