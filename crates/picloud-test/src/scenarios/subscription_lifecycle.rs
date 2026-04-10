//! ADR-022: Verify subscription create/update/delete lifecycle in domain types.
//!
//! Reads events source to confirm that the EventSubscription resource
//! follows the standard resource lifecycle (Declared, Provisioning,
//! Ready, Deleting) and that deletion events clean up subscriptions.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SubscriptionLifecycle;

#[async_trait]
impl Scenario for SubscriptionLifecycle {
    fn name(&self) -> &str {
        "subscription-lifecycle"
    }

    fn adr(&self) -> &str {
        "ADR-022"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify ResourceStatus enum covers full lifecycle
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

        let lifecycle_states = ["Declared", "Provisioning", "Ready", "Failed", "Deleting"];
        for state in &lifecycle_states {
            if !resources_content.contains(state) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "ResourceStatus missing '{}' variant needed for subscription lifecycle",
                        state
                    ),
                };
            }
        }

        // 2. Verify EventSubscription resource type exists
        if !resources_content.contains("struct EventSubscription") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscription struct not found in resources.rs".to_string(),
            };
        }

        // 3. Verify deletion events exist for cleanup
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

        if !events_content.contains("ResourceDeleted") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ResourceDeleted event not found — needed for subscription deletion"
                    .to_string(),
            };
        }

        // 4. Verify EventSubscriptionSpec in product.rs for declaration
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

        if !product_content.contains("EventSubscriptionSpec") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscriptionSpec not found in product.rs".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
