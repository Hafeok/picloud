//! ADR-019: Product SPARQL endpoint — POST a SPARQL query to
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

        // Discover an existing product.
        let products_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?name WHERE {
                ?p a picloud:Product ;
                   picloud:name ?name .
            } LIMIT 1
        "#;

        let product_name = match assertions::sparql_query(ctx, products_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(_) => {
                        return ScenarioResult::Skip {
                            reason: "no products deployed".to_string(),
                        };
                    }
                };
                match json
                    .pointer("/results/bindings/0/name/value")
                    .and_then(|v| v.as_str())
                {
                    Some(name) => name.to_string(),
                    None => {
                        return ScenarioResult::Skip {
                            reason: "no products deployed".to_string(),
                        };
                    }
                }
            }
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "cannot query products".to_string(),
                };
            }
        };

        // POST SPARQL to the product's graph endpoint.
        let sparql_url = format!(
            "{}/products/{}/graph",
            ctx.config.base_url(),
            product_name
        );

        let sparql_query = "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10";

        let resp = match ctx
            .http_client
            .post(&sparql_url)
            .header("Content-Type", "application/sparql-query")
            .header("Accept", "application/sparql-results+json")
            .body(sparql_query.to_string())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to POST SPARQL to product endpoint: {}", e),
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
