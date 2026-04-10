//! ADR-036: Tag RDF projection — SPARQL for tags on resources.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TagRdfProjection;

#[async_trait]
impl Scenario for TagRdfProjection {
    fn name(&self) -> &str {
        "tag-rdf-projection"
    }

    fn adr(&self) -> &str {
        "ADR-036"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query for any tags in the graph.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?resource ?tag WHERE {
                ?resource picloud:hasTag ?tag .
            } LIMIT 10
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

                // The query itself should execute successfully even if no tags exist yet.
                if json.pointer("/results/bindings").is_some() {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "SPARQL response missing /results/bindings".to_string(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("tag SPARQL query failed: {}", e),
            },
        }
    }
}
