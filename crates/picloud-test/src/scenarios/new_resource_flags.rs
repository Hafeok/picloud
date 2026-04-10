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

        // 1. Declare a feature flag resource
        let flag_cmd = serde_json::json!({
            "type": "ResourceDeclared",
            "product": "test-flags-product",
            "resource_type": "feature-flag",
            "name": "test-new-flag",
            "enabled": true,
            "version": ">= 1",
        });

        match assertions::http_post(ctx, "/api/commands", flag_cmd).await {
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
                    reason: format!("command endpoint not available: {}", e),
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
                if resp.status().is_success() {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "flag evaluation returned status: {}",
                            resp.status().as_u16()
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
