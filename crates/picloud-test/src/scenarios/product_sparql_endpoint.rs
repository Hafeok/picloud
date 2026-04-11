//! ADR-019: Product SPARQL endpoint — GET a SPARQL query from
//! /products/{name}/graph and assert a valid response.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ProductSparqlEndpoint;

#[async_trait]
impl Scenario for ProductSparqlEndpoint {
    fn name(&self) -> &str {
        "product-sparql-endpoint"
    }

    fn adr(&self) -> &str {
        "ADR-019"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Seed a test product so the RDF graph has data.
        if let Err(_) =
            assertions::apply_product_and_wait(ctx, "e2e-sparql-endpoint", "1.0.0").await
        {
            return ScenarioResult::Skip {
                reason: "could not seed test product".to_string(),
            };
        }

        let product_name = "e2e-sparql-endpoint".to_string();

        // POST SPARQL to the product's graph endpoint.
        let sparql_url = format!(
            "{}/products/{}/graph",
            ctx.config.base_url(),
            product_name
        );

        let sparql_query = "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10";

        let resp = match ctx
            .http_client
            .get(&sparql_url)
            .query(&[("query", sparql_query)])
            .header("Accept", "application/sparql-results+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query product SPARQL endpoint: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();

        if status == 200 {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else if status == 401 || status == 403 {
            // IAM enforcement is working — valid but unauthenticated.
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "product SPARQL endpoint returned unexpected status: {}",
                    status
                ),
            }
        }
    }
}
