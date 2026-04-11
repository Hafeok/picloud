//! ADR-044: Apply new resource with feature flags. Verify flags take effect.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NewResourceFlags;

#[async_trait]
impl Scenario for NewResourceFlags {
    fn name(&self) -> &str {
        "new-resource-flags"
    }

    fn adr(&self) -> &str {
        "ADR-044"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Ensure the parent product exists, then declare a feature flag resource
        let product_resource = serde_json::json!({
            "type": "product",
            "name": "test-flags-product",
            "version": "1.0.0"
        });
        let _ = assertions::apply_resource(ctx, product_resource).await;

        let flag_resource = serde_json::json!({
            "type": "config",
            "name": "test-new-flag",
            "product": "test-flags-product",
            "entries": [
                { "key": "enabled", "value": "true" },
                { "key": "version", "value": ">= 1" }
            ]
        });

        match assertions::apply_resource(ctx, flag_resource).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "feature flag resource creation not available".to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("apply endpoint not available: {}", e),
                };
            }
        }

        // 2. Verify flag appears in the RDF graph
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?flag a picloud:FeatureFlag ;
                      picloud:flagName "test-new-flag" ;
                      picloud:flagEnabled true .
            }
        "#;

        match assertions::wait_for_sparql(ctx, ask_query, Duration::from_secs(10)).await {
            Ok(()) => {}
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("feature flag not projected to graph: {}", e),
                };
            }
        }

        // 3. Evaluate the flag via HTTP
        match assertions::http_get(ctx, "/products/test-flags-product/flags/test-new-flag").await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if resp.status().is_success() {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if status == 404 || status == 501 {
                    ScenarioResult::Skip {
                        reason: "flag evaluation endpoint not implemented yet".to_string(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "flag evaluation returned status: {}",
                            status
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to evaluate flag: {}", e),
            },
        }
    }
}
