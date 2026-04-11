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

        // Seed a test product so the RDF graph has data.
        if let Err(_) =
            assertions::apply_product_and_wait(ctx, "e2e-sparql-iam", "1.0.0").await
        {
            return ScenarioResult::Skip {
                reason: "could not seed test product".to_string(),
            };
        }

        let product_name = "e2e-sparql-iam".to_string();

        // Query the product SPARQL endpoint WITHOUT an auth token.
        let sparql_url = format!(
            "{}/products/{}/graph",
            ctx.config.base_url(),
            product_name
        );

        let resp = match ctx
            .http_client
            .get(&sparql_url)
            .query(&[("query", "SELECT ?s WHERE { ?s ?p ?o } LIMIT 1")])
            .header("Accept", "application/sparql-results+json")
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
