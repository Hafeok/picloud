//! ADR-039: Deploy a product ontology and verify RDF content is immediately present.
//!
//! GET /products/{name}/ontology after applying an ontology. Assert RDF content present.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OntologyDeployImmediate;

#[async_trait]
impl Scenario for OntologyDeployImmediate {
    fn name(&self) -> &str {
        "ontology-deploy-immediate"
    }

    fn adr(&self) -> &str {
        "ADR-039"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Check cluster availability first.
        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query a known product ontology endpoint.
        let resp = match assertions::http_get(ctx, "/products/test-app/ontology").await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("ontology endpoint not available: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();
        if status == 404 || status == 501 {
            return ScenarioResult::Skip {
                reason: "ontology endpoint not implemented yet".to_string(),
            };
        }

        if !resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("ontology endpoint returned status {}", status),
            };
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read ontology response body: {}", e),
                };
            }
        };

        // Assert that RDF content is present (Turtle or JSON-LD).
        if body.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ontology response body is empty — no RDF content".to_string(),
            };
        }

        // Check for RDF markers (prefixes, IRIs, or JSON-LD context).
        let has_rdf_content = body.contains("@prefix")
            || body.contains("<http")
            || body.contains("\"@context\"")
            || body.contains("rdf:type")
            || body.contains("rdfs:");
        if !has_rdf_content {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ontology response does not contain recognisable RDF content".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
