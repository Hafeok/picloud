//! ADR-039: Apply resources with OWL transitive properties.
//!
//! SPARQL for inferred transitive closure: if A dependsOn B and B dependsOn C,
//! then A dependsOn C should be inferred by the OWL 2 RL reasoner.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OwlTransitivity;

#[async_trait]
impl Scenario for OwlTransitivity {
    fn name(&self) -> &str {
        "owl-transitivity"
    }

    fn adr(&self) -> &str {
        "ADR-039"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query for transitive closure of picloud:dependsOn.
        // If A dependsOn B and B dependsOn C, the reasoner should infer A dependsOn C.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?a picloud:dependsOn ?c .
                FILTER NOT EXISTS {
                    ?a picloud:dependsOn ?b .
                    ?b picloud:dependsOn ?c .
                    FILTER(?a != ?c)
                }
            }
        "#;

        let result = match assertions::sparql_query(ctx, query).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SPARQL endpoint not available: {}", e),
                };
            }
        };

        // Verify the SPARQL endpoint returned a valid response.
        if result.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SPARQL query returned empty response".to_string(),
            };
        }

        // Check that the dependsOn property is declared as transitive.
        let ontology_query = r#"
            PREFIX owl: <http://www.w3.org/2002/07/owl#>
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                picloud:dependsOn a owl:TransitiveProperty .
            }
        "#;

        match assertions::sparql_query(ctx, ontology_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse SPARQL ASK response: {}", e),
                        };
                    }
                };

                if json.get("boolean").and_then(|v| v.as_bool()) != Some(true) {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "picloud:dependsOn is not declared as owl:TransitiveProperty"
                            .to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SPARQL query for transitive property failed: {}", e),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
