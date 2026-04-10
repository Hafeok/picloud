//! ADR-036: Tag SPARQL queryable — SPARQL FILTER by tag key/value and
//! assert correct results.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TagSparqlQueryable;

#[async_trait]
impl Scenario for TagSparqlQueryable {
    fn name(&self) -> &str {
        "tag-sparql-queryable"
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

        // Query for resources with a specific tag pattern using FILTER.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?resource ?key ?value WHERE {
                ?resource picloud:tag ?tagNode .
                ?tagNode picloud:tagKey ?key ;
                         picloud:tagValue ?value .
                FILTER(?key = "environment")
            } LIMIT 20
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

                // Verify the response has the correct structure.
                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array());

                match bindings {
                    Some(arr) => {
                        // All returned results should have key = "environment".
                        for binding in arr {
                            let key = binding
                                .pointer("/key/value")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if key != "environment" {
                                return ScenarioResult::Fail {
                                    duration: start.elapsed(),
                                    reason: format!(
                                        "FILTER returned tag with key '{}', expected 'environment'",
                                        key
                                    ),
                                };
                            }
                        }
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    }
                    None => ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "SPARQL response missing /results/bindings".to_string(),
                    },
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("tag SPARQL FILTER query failed: {}", e),
            },
        }
    }
}
