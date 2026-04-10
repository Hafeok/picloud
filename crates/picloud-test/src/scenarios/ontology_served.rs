//! ADR-019: Ontology served — GET /products/{name}/ontology and assert
//! the response contains RDF content.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OntologyServed;

#[async_trait]
impl Scenario for OntologyServed {
    fn name(&self) -> &str {
        "ontology-served"
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

        // Discover a product to query its ontology.
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
                            reason: "no products deployed to test ontology serving"
                                .to_string(),
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
                            reason: "no products deployed to test ontology serving"
                                .to_string(),
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

        // GET the product's ontology endpoint.
        let path = format!("/products/{}/ontology", product_name);
        match assertions::http_get(ctx, &path).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let content_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let body = resp.text().await.unwrap_or_default();

                if status != 200 {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "ontology endpoint returned {} (expected 200)",
                            status
                        ),
                    };
                }

                if body.trim().is_empty() {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "ontology response body is empty".to_string(),
                    };
                }

                // Expect RDF content type (Turtle, JSON-LD, or similar).
                let is_rdf = content_type.contains("turtle")
                    || content_type.contains("ld+json")
                    || content_type.contains("rdf")
                    || content_type.contains("n-triples");

                if !is_rdf && !body.contains("@prefix") && !body.contains("@context") {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "ontology response does not appear to be RDF (content-type: {})",
                            content_type
                        ),
                    };
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to GET ontology: {}", e),
            },
        }
    }
}
