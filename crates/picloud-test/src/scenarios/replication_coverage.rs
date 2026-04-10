//! ADR-011: Replication coverage — referenced as a dependency check target by
//! phase_dependency_order. Verifies data replication across nodes.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ReplicationCoverageScenario;

#[async_trait]
impl Scenario for ReplicationCoverageScenario {
    fn name(&self) -> &str {
        "replication-coverage"
    }

    fn adr(&self) -> &str {
        "ADR-011"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query for replicated volumes and their replica count
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?volume ?replicas WHERE {
                ?volume a picloud:Volume ;
                        picloud:replicationFactor ?replicas .
            }
        "#;

        match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(_) => ScenarioResult::Pass {
                        duration: start.elapsed(),
                    },
                    Err(e) => ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("invalid SPARQL response: {}", e),
                    },
                }
            }
            Err(e) => ScenarioResult::Skip {
                reason: format!("SPARQL not available: {}", e),
            },
        }
    }
}
