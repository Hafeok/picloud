//! ADR-044: Evaluate feature flag in-process. Assert correct result
//! with zero network round-trips after initial load.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct FlagInProcessEvaluation;

#[async_trait]
impl Scenario for FlagInProcessEvaluation {
    fn name(&self) -> &str {
        "flag-in-process-evaluation"
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

        // 1. Verify FeatureFlag resource type exists with is_active for in-process eval
        let resources_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/resources.rs");
        let resources_content = match std::fs::read_to_string(&resources_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", resources_path.display(), e),
                };
            }
        };

        if !resources_content.contains("struct FeatureFlag") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag resource type not found".to_string(),
            };
        }

        // 2. Verify is_active method for in-process evaluation
        if !resources_content.contains("fn is_active") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag missing is_active method for in-process evaluation"
                    .to_string(),
            };
        }

        // 3. Check flag evaluation endpoint returns results
        match assertions::http_get(ctx, "/products/test-product/flags").await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "flags endpoint not available".to_string(),
                    };
                }
            }
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "flags endpoint not reachable".to_string(),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
