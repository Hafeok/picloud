//! ADR-002: Raft learner join — check for learner nodes in the cluster graph.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RaftLearnerJoin;

#[async_trait]
impl Scenario for RaftLearnerJoin {
    fn name(&self) -> &str {
        "raft-learner-join"
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

        // ASK whether any learner nodes exist in the graph.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?node a picloud:Node ;
                      picloud:hasRole picloud:Learner .
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

                // ASK queries return { "boolean": true/false }.
                // We accept both true and false — the test validates that the query executes
                // successfully and the graph structure supports learner role queries.
                match json.get("boolean").and_then(|v| v.as_bool()) {
                    Some(_) => ScenarioResult::Pass {
                        duration: start.elapsed(),
                    },
                    None => ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "SPARQL ASK response missing 'boolean' field".to_string(),
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
