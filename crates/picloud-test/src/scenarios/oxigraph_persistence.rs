//! ADR-006: Oxigraph persistence — query triple count and verify > 0,
//! confirming the server has persisted state.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OxigraphPersistence;

#[async_trait]
impl Scenario for OxigraphPersistence {
    fn name(&self) -> &str {
        "oxigraph-persistence"
    }

    fn adr(&self) -> &str {
        "ADR-006"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Count all triples in the store. A running server with state should have > 0.
        let query = r#"
            SELECT (COUNT(*) AS ?count) WHERE { ?s ?p ?o }
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

                let count = json
                    .pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);

                if count > 0 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "triple count is 0 — Oxigraph has no persisted state"
                            .to_string(),
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
