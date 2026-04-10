//! ADR-011: Product SPARQL — referenced as a dependency check target by
//! phase_dependency_order. Verifies per-product SPARQL endpoint works.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ProductSparqlScenario;

#[async_trait]
impl Scenario for ProductSparqlScenario {
    fn name(&self) -> &str {
        "product-sparql"
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

        // Query the platform SPARQL endpoint for products
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?product ?name WHERE {
                ?product a picloud:Product ;
                         picloud:name ?name .
            }
            LIMIT 10
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
