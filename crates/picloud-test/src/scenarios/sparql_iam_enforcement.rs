//! ADR-019: SPARQL IAM enforcement — query SPARQL without auth token
//! and assert 401 or 403.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SparqlIamEnforcement;

#[async_trait]
impl Scenario for SparqlIamEnforcement {
    fn name(&self) -> &str {
        "sparql-iam-enforcement"
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

        // Discover a product with a SPARQL endpoint.
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

        // Query the product SPARQL endpoint WITHOUT an auth token.
        let sparql_url = format!(
            "{}/products/{}/graph",
            ctx.config.base_url(),
            product_name
        );

        let resp = match ctx
            .http_client
            .post(&sparql_url)
            .header("Content-Type", "application/sparql-query")
            .body("SELECT ?s WHERE { ?s ?p ?o } LIMIT 1".to_string())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("request failed: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();

        if status == 401 || status == 403 {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "expected 401 or 403 for unauthenticated SPARQL, got {}",
                    status
                ),
            }
        }
    }
}
