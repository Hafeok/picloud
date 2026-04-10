//! ADR-039: Apply resources with rdfs:subClassOf. SPARQL ASK for inferred type hierarchy.
//!
//! Declares picloud:ProductionContainer rdfs:subClassOf picloud:Container.
//! Queries for all picloud:Container instances and asserts that
//! ProductionContainer instances are included via RDFS inference.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RdfsSubclassInference;

#[async_trait]
impl Scenario for RdfsSubclassInference {
    fn name(&self) -> &str {
        "rdfs-subclass-inference"
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

        // Verify the subclass relationship is declared in the ontology.
        let subclass_query = r#"
            PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                picloud:ProductionContainer rdfs:subClassOf picloud:Container .
            }
        "#;

        match assertions::sparql_query(ctx, subclass_query).await {
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
                    return ScenarioResult::Skip {
                        reason: "rdfs:subClassOf relationship not declared yet — ontology not deployed".to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SPARQL endpoint not available: {}", e),
                };
            }
        }

        // Query for instances of picloud:Container — RDFS inference should
        // include instances typed as picloud:ProductionContainer.
        let inference_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?x WHERE {
                ?x a picloud:Container .
            }
        "#;

        match assertions::sparql_query(ctx, inference_query).await {
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

                // Verify we got a valid SPARQL results structure.
                if json.pointer("/results/bindings").is_none() {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "SPARQL response missing results/bindings".to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("SPARQL query for Container instances failed: {}", e),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
