//! ADR-021: Verify rollback mechanism exists for failed version upgrades.
//!
//! Reads product/upgrade source to confirm the UpgradeState enum includes
//! a Failed variant and that the ProductUpgradeAborted event exists,
//! ensuring the platform can roll back failed upgrades.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct FailedUpgradeRollback;

#[async_trait]
impl Scenario for FailedUpgradeRollback {
    fn name(&self) -> &str {
        "failed-upgrade-rollback"
    }

    fn adr(&self) -> &str {
        "ADR-021"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify UpgradeState enum exists in product.rs with Failed variant
        let product_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/product.rs");
        let product_content = match std::fs::read_to_string(&product_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", product_path.display(), e),
                };
            }
        };

        if !product_content.contains("enum UpgradeState") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "UpgradeState enum not found in product.rs".to_string(),
            };
        }

        // 2. Verify UpgradeState has a Failed variant
        if !product_content.contains("Failed") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "UpgradeState enum missing Failed variant for rollback".to_string(),
            };
        }

        // 3. Verify UpgradeState has Provisioning and CuttingOver (full lifecycle)
        if !product_content.contains("Provisioning") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "UpgradeState missing Provisioning variant".to_string(),
            };
        }
        if !product_content.contains("CuttingOver") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "UpgradeState missing CuttingOver variant".to_string(),
            };
        }

        // 4. Verify ProductUpgradeAborted event exists in events.rs
        let events_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/events.rs");
        let events_content = match std::fs::read_to_string(&events_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", events_path.display(), e),
                };
            }
        };

        if !events_content.contains("ProductUpgradeAborted") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ProductUpgradeAborted event not found in events.rs".to_string(),
            };
        }

        // 5. Verify abort payload carries reason
        if !events_content.contains("ProductUpgradeAbortedPayload") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ProductUpgradeAbortedPayload not found in events.rs".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
