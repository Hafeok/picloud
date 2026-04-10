//! ADR-022: Verify validation rejects subscriptions to undeclared event schemas.
//!
//! Reads events and resources source to confirm that the EventSubscription
//! resource type requires an explicit event_type declaration, and that the
//! domain model enforces this constraint structurally — subscriptions without
//! a declared resource are not permitted.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct UndeclaredSubscriptionRejection;

#[async_trait]
impl Scenario for UndeclaredSubscriptionRejection {
    fn name(&self) -> &str {
        "undeclared-subscription-rejection"
    }

    fn adr(&self) -> &str {
        "ADR-022"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify EventSubscription requires event_type (not optional)
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

        // event_type must be a required String field, not Option<String>
        if !resources_content.contains("pub event_type: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "EventSubscription.event_type must be a required String (not Optional) to enforce declaration"
                        .to_string(),
            };
        }

        // source_product_iri must also be required
        if !resources_content.contains("pub source_product_iri: ResourceIri") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "EventSubscription.source_product_iri must be a required field to enforce declaration"
                        .to_string(),
            };
        }

        // 2. Verify EventSubscriptionSpec also has required fields
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

        if !product_content.contains("struct EventSubscriptionSpec") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "EventSubscriptionSpec not found in product.rs".to_string(),
            };
        }

        // The spec must have event_type as a required field
        if !product_content.contains("pub event_type: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "EventSubscriptionSpec.event_type must be required to reject undeclared subscriptions"
                        .to_string(),
            };
        }

        // 3. Verify handler field exists — subscriptions must target a handler
        if !resources_content.contains("pub handler_name: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "EventSubscription.handler_name must be required to bind subscription to a handler"
                        .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
