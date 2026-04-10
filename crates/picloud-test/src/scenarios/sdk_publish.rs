//! ADR-033: Check if SDK publish endpoint exists. Verify generated SDK artifacts.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SdkPublish;

#[async_trait]
impl Scenario for SdkPublish {
    fn name(&self) -> &str {
        "sdk-publish"
    }

    fn adr(&self) -> &str {
        "ADR-033"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check SDK publish endpoint exists
        if !assertions::feature_available(ctx, "/api/sdk/publish").await {
            return ScenarioResult::Skip {
                reason: "SDK publish endpoint not available".to_string(),
            };
        }

        // 2. Verify picloud-sdk-gen crate has publish capability
        let impl_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/src/implementation.rs");
        let impl_content = match std::fs::read_to_string(&impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", impl_path.display(), e),
                };
            }
        };

        // 3. Verify publish method exists
        if !impl_content.contains("fn publish") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator missing publish method".to_string(),
            };
        }

        // 4. Verify SdkPublishResult type exists
        if !impl_content.contains("SdkPublishResult") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkPublishResult type not found".to_string(),
            };
        }

        // 5. Verify registry targets (crates.io, npm, nuget)
        let registries = ["crates.io", "npm", "nuget"];
        for registry in &registries {
            if !impl_content.contains(registry) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("SDK publish missing registry target: {}", registry),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
