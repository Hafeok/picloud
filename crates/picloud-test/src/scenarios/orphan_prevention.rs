//! ADR-016: Orphan prevention — try to apply a container without a parent
//! product and assert the server rejects it.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OrphanPrevention;

#[async_trait]
impl Scenario for OrphanPrevention {
    fn name(&self) -> &str {
        "orphan-prevention"
    }

    fn adr(&self) -> &str {
        "ADR-016"
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

        // Attempt to apply a container resource without a valid parent product.
        let orphan_resource = serde_json::json!({
            "type": "container",
            "name": "orphan-container",
            "product": "nonexistent-product-orphan-test",
            "image": "test:latest"
        });

        match assertions::apply_resource(ctx, orphan_resource).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                // Expect 400 or 404 — the parent product does not exist.
                if status == 400 || status == 404 || status == 422 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expected 400/404/422 for orphan resource, got {}",
                            status
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST orphan command: {}", e),
            },
        }
    }
}
