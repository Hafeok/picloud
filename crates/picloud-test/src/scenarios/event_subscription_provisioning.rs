//! ADR-022: Verify subscription resource types exist and can be provisioned via events.
//!
//! Reads domain source to confirm the EventSubscription resource type
//! is declared, has the correct fields, and that the resource lifecycle
//! events (ResourceDeclared, ResourceReady) support it.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventSubscriptionProvisioning;

#[async_trait]
impl Scenario for EventSubscriptionProvisioning {
    fn name(&self) -> &str {
        "event-subscription-provisioning"
    }

    fn adr(&self) -> &str {
        "ADR-022"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify EventSubscription resource type exists
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

        if !resources_content.contains("struct EventSubscription") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscription resource type not found in resources.rs".to_string(),
            };
        }

        // 2. Verify required fields for provisioning
        let required_fields = [
            "source_product_iri",
            "event_type",
            "handler_name",
        ];
        for field in &required_fields {
            if !resources_content.contains(field) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "EventSubscription missing required field '{}' for provisioning",
                        field
                    ),
                };
            }
        }

        // 3. Verify ResourceMeta is included (for lifecycle management via events)
        if !resources_content.contains("pub meta: ResourceMeta") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscription likely missing ResourceMeta for lifecycle events"
                    .to_string(),
            };
        }

        // 4. Verify ResourceDeclared and ResourceReady events exist for provisioning
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

        if !events_content.contains("ResourceDeclared") || !events_content.contains("ResourceReady")
        {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ResourceDeclared/ResourceReady events missing for subscription provisioning"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
