//! ADR-036: Verify TagAdded event type exists with correct fields.
//!
//! Reads domain events source to confirm that the TagAdded event variant
//! and TagAddedPayload exist with the required fields: resource_iri, key, value.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TagAddEvent;

#[async_trait]
impl Scenario for TagAddEvent {
    fn name(&self) -> &str {
        "tag-add-event"
    }

    fn adr(&self) -> &str {
        "ADR-036"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

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

        // 1. Verify TagAdded variant exists in PlatformEvent enum
        if !events_content.contains("TagAdded(TagAddedPayload)") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TagAdded variant not found in PlatformEvent enum".to_string(),
            };
        }

        // 2. Verify TagAddedPayload struct exists
        if !events_content.contains("struct TagAddedPayload") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TagAddedPayload struct not found in events.rs".to_string(),
            };
        }

        // 3. Verify required fields on TagAddedPayload
        if !events_content.contains("pub resource_iri: ResourceIri") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TagAddedPayload missing resource_iri field".to_string(),
            };
        }

        // Check for key and value fields (they should be String type)
        // Look in the TagAddedPayload section specifically
        let payload_section = if let Some(idx) = events_content.find("struct TagAddedPayload") {
            &events_content[idx..idx + 300.min(events_content.len() - idx)]
        } else {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Could not locate TagAddedPayload section".to_string(),
            };
        };

        if !payload_section.contains("pub key: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TagAddedPayload missing 'key' field".to_string(),
            };
        }

        if !payload_section.contains("pub value: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TagAddedPayload missing 'value' field".to_string(),
            };
        }

        // 4. Verify TagRemoved also exists (complete tag lifecycle)
        if !events_content.contains("TagRemoved(TagRemovedPayload)") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "TagRemoved variant not found — tag lifecycle incomplete".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
