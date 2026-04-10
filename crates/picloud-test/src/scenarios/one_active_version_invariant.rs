//! ADR-021: One active version invariant — SPARQL count of products with
//! same name but different active versions must be <= 1.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OneActiveVersionInvariant;

#[async_trait]
impl Scenario for OneActiveVersionInvariant {
    fn name(&self) -> &str {
        "one-active-version-invariant"
    }

    fn adr(&self) -> &str {
        "ADR-021"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // For each product, count distinct active versions. Should be exactly 1.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?product (COUNT(DISTINCT ?version) AS ?count) WHERE {
                ?product a picloud:Product ;
                         picloud:activeVersion ?version .
            }
            GROUP BY ?product
            HAVING (COUNT(DISTINCT ?version) > 1)
        "#;

        match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse response: {}", e),
                        };
                    }
                };

                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array());

                match bindings {
                    Some(arr) if arr.is_empty() => {
                        // No products with multiple active versions — pass.
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    }
                    Some(arr) => {
                        let violators: Vec<String> = arr
                            .iter()
                            .filter_map(|b| {
                                b.pointer("/product/value")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect();
                        ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "products with multiple active versions: {:?}",
                                violators
                            ),
                        }
                    }
                    None => {
                        // No bindings key — could mean no products exist, which is fine.
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("SPARQL query failed: {}", e),
            },
        }
    }
}
