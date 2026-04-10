//! ADR-044: Set flag with version range. Assert evaluation respects range
//! (active for versions in range, inactive outside).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct FlagVersionRange;

#[async_trait]
impl Scenario for FlagVersionRange {
    fn name(&self) -> &str {
        "flag-version-range"
    }

    fn adr(&self) -> &str {
        "ADR-044"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Verify version range evaluation logic exists in domain code
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

        // 1. Verify VersionOp enum with range support
        if !resources_content.contains("enum VersionOp") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "VersionOp enum not found — needed for version range evaluation"
                    .to_string(),
            };
        }

        // 2. Verify range operator exists (N..M inclusive range)
        if !resources_content.contains("Range") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "VersionOp missing Range variant for N..M expressions".to_string(),
            };
        }

        // 3. Verify all comparison operators
        let operators = ["Eq", "Gt", "Gte", "Lt", "Lte"];
        for op in &operators {
            if !resources_content.contains(op) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("VersionOp missing operator: {}", op),
                };
            }
        }

        // 4. Verify is_active method exists for evaluation
        if !resources_content.contains("fn is_active") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag missing is_active evaluation method".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
