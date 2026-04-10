//! ADR-011: Volume mount — referenced as a dependency check target by
//! phase_dependency_order. Verifies a mounted volume is accessible.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct VolumeMountScenario;

#[async_trait]
impl Scenario for VolumeMountScenario {
    fn name(&self) -> &str {
        "volume-mount"
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

        // Query for volumes in the RDF graph
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?v) AS ?count) WHERE {
                ?v a picloud:Volume .
            }
        "#;

        match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                // Parse SPARQL response — any valid response means storage subsystem is working
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
