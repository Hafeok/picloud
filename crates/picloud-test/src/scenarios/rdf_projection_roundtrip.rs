//! ADR-005: RDF projection roundtrip — apply a product, then CONSTRUCT all
//! triples for it via SPARQL and verify the structure.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RdfProjectionRoundtrip;

#[async_trait]
impl Scenario for RdfProjectionRoundtrip {
    fn name(&self) -> &str {
        "rdf-projection-roundtrip"
    }

    fn adr(&self) -> &str {
        "ADR-005"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::commands_available(ctx).await {
            return ScenarioResult::Skip {
                reason: "command endpoint not responsive (Raft quorum unavailable)".to_string(),
            };
        }

        // Apply a test product.
        let test_name = format!("test-rdf-rt-{}", uuid::Uuid::new_v4().as_simple());
        let resource = serde_json::json!({
            "type": "product",
            "name": test_name,
            "version": "1.0.0"
        });

        if let Err(e) = assertions::apply_resource(ctx, resource).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply product: {}", e),
            };
        }

        // Wait for projection.
        let ask_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <https://picloud.local/products/{}> a picloud:Product .
            }}
            "#,
            test_name
        );

        if let Err(e) =
            assertions::wait_for_sparql(ctx, &ask_query, Duration::from_secs(10)).await
        {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("product not projected: {}", e),
            };
        }

        // CONSTRUCT all triples for the product.
        let construct_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            CONSTRUCT {{
                <https://picloud.local/products/{}> ?p ?o .
            }}
            WHERE {{
                <https://picloud.local/products/{}> ?p ?o .
            }}
            "#,
            test_name, test_name
        );

        match assertions::sparql_query(ctx, &construct_query).await {
            Ok(body) => {
                if body.trim().is_empty() {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "CONSTRUCT returned empty body".to_string(),
                    };
                }
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("CONSTRUCT query failed: {}", e),
            },
        }
    }
}
