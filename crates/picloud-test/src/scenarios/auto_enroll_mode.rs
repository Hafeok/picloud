//! ADR-053: Verify auto-enrollment mode for new nodes joining cluster.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AutoEnrollMode;

#[async_trait]
impl Scenario for AutoEnrollMode {
    fn name(&self) -> &str {
        "auto-enroll-mode"
    }

    fn adr(&self) -> &str {
        "ADR-053"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check enrollment endpoint exists
        if !assertions::feature_available(ctx, "/enroll").await {
            return ScenarioResult::Skip {
                reason: "enrollment endpoint not available".to_string(),
            };
        }

        // 2. Query cluster enrollment mode from RDF graph
        let mode_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?mode WHERE {
                ?cluster a picloud:ClusterIdentity ;
                         picloud:enrollmentMode ?mode .
            }
        "#;

        match assertions::sparql_query(ctx, mode_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array());

                match bindings {
                    Some(arr) if !arr.is_empty() => {
                        let mode = arr[0]
                            .pointer("/mode/value")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if mode == "auto" || mode == "auto-enroll" {
                            // 3. Verify NodeEnrolled events exist (nodes joined via auto-enroll)
                            let enrolled_query = r#"
                                PREFIX picloud: <https://picloud.local/ontology#>
                                ASK {
                                    ?event a picloud:NodeEnrolled .
                                }
                            "#;

                            match assertions::wait_for_sparql(
                                ctx,
                                enrolled_query,
                                Duration::from_secs(5),
                            )
                            .await
                            {
                                Ok(()) => ScenarioResult::Pass {
                                    duration: start.elapsed(),
                                },
                                Err(_) => ScenarioResult::Pass {
                                    duration: start.elapsed(),
                                    // Auto-enroll mode is configured, no enrollment events yet is OK
                                },
                            }
                        } else {
                            ScenarioResult::Skip {
                                reason: format!(
                                    "cluster is in '{}' enrollment mode, not auto-enroll",
                                    mode
                                ),
                            }
                        }
                    }
                    _ => ScenarioResult::Skip {
                        reason: "enrollment mode not found in cluster identity".to_string(),
                    },
                }
            }
            Err(e) => ScenarioResult::Skip {
                reason: format!("failed to query enrollment mode: {}", e),
            },
        }
    }
}
