//! ADR-018: Verify inter-product event delivery types exist.
//!
//! Reads picloud-domain events source and picloud-events source to verify
//! that event bus types exist for inter-product events. Checks that
//! ProductEventStore has cross-product subscription support via the
//! EventSubscription resource type.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct InterProductEventDelivery;

#[async_trait]
impl Scenario for InterProductEventDelivery {
    fn name(&self) -> &str {
        "inter-product-event-delivery"
    }

    fn adr(&self) -> &str {
        "ADR-018"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify EventSubscription resource type exists in picloud-domain/src/resources.rs
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

        // 2. Verify EventSubscription has source_product_iri and event_type fields
        if !resources_content.contains("source_product_iri") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscription missing source_product_iri field".to_string(),
            };
        }

        if !resources_content.contains("event_type") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscription missing event_type field".to_string(),
            };
        }

        // 3. Verify EventSubscriptionSpec in product.rs for cross-product subscription
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

        // 4. Verify the EventLog trait exists in traits.rs for event bus abstraction
        let traits_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/traits.rs");
        let traits_content = match std::fs::read_to_string(&traits_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", traits_path.display(), e),
                };
            }
        };

        if !traits_content.contains("trait EventLog") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventLog trait not found in traits.rs".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
