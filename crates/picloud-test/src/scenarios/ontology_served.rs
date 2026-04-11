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

        // Seed a test product so the RDF graph has data.
        if let Err(_) = assertions::apply_product_and_wait(ctx, "e2e-ontology-test", "1.0.0").await
        {
            return ScenarioResult::Skip {
                reason: "could not seed test product".to_string(),
            };
        }

        let product_name = "e2e-ontology-test".to_string();

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
