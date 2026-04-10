//! ADR-005: Graph isolation — verify platform and product named graphs
//! do not leak triples to each other.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct GraphIsolation;

#[async_trait]
impl Scenario for GraphIsolation {
    fn name(&self) -> &str {
        "graph-isolation"
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

        // Query platform graph for product-scoped triples — should find none.
        let platform_leak_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?s) AS ?count) WHERE {
                GRAPH <https://picloud.local/graph/platform> {
                    ?s a picloud:ProductResource .
                }
            }
        "#;

        match assertions::assert_sparql_count(ctx, platform_leak_query, 0).await {
            Ok(()) => {}
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "platform graph contains product-scoped triples: {}",
                        e
                    ),
                };
            }
        }

        // Query a product named graph for platform-level triples — should find none.
        let product_leak_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?s) AS ?count) WHERE {
                GRAPH <https://picloud.local/graph/products> {
                    ?s a picloud:Node .
                }
            }
        "#;

        match assertions::assert_sparql_count(ctx, product_leak_query, 0).await {
            Ok(()) => {}
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "product graph contains platform-level triples: {}",
                        e
                    ),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
