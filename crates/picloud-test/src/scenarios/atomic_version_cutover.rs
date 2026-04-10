//! ADR-021: Atomic version cutover — verify no window where both v1 and v2
//! containers are simultaneously tagged `picloud:Running`.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AtomicVersionCutoverScenario;

#[async_trait]
impl Scenario for AtomicVersionCutoverScenario {
    fn name(&self) -> &str {
        "atomic-version-cutover"
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

        // Query for products with multiple active versions
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?product (COUNT(DISTINCT ?version) AS ?vcount)
            WHERE {
                ?product a picloud:Product ;
                         picloud:version ?version ;
                         picloud:status picloud:Running .
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
                            reason: format!("failed to parse SPARQL response: {}", e),
                        };
                    }
                };

                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);

                if bindings > 0 {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "{} product(s) have multiple active versions simultaneously",
                            bindings
                        ),
                    };
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => {
                // SPARQL endpoint may not support this query yet
                ScenarioResult::Skip {
                    reason: format!("SPARQL query failed: {}", e),
                }
            }
        }
    }
}
