//! ADR-038: Verify inference rule resource types exist.
//!
//! Reads domain/RDF source to confirm that the InferenceRule resource type
//! is declared with the required fields (scope, trigger, construct_query,
//! reconciliation) and that the associated events exist.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct InferenceRuleLifecycle;

#[async_trait]
impl Scenario for InferenceRuleLifecycle {
    fn name(&self) -> &str {
        "inference-rule-lifecycle"
    }

    fn adr(&self) -> &str {
        "ADR-038"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify InferenceRule resource type exists
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

        if !resources_content.contains("struct InferenceRule") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "InferenceRule resource type not found in resources.rs".to_string(),
            };
        }

        // 2. Verify required fields
        let required_fields = [
            ("scope", "Scope field (platform or product name)"),
            ("trigger", "Trigger mode field"),
            ("construct_query", "SPARQL CONSTRUCT query field"),
            ("reconciliation", "Reconciliation flag"),
            ("trigger_events", "Trigger events list"),
        ];

        for (field, desc) in &required_fields {
            if !resources_content.contains(field) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("InferenceRule missing field: {} ({})", field, desc),
                };
            }
        }

        // 3. Verify InferenceRuleEvaluated event exists
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

        if !events_content.contains("InferenceRuleEvaluated") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "InferenceRuleEvaluated event not found in events.rs".to_string(),
            };
        }

        // 4. Verify ReconciliationCompleted event exists
        if !events_content.contains("ReconciliationCompleted") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ReconciliationCompleted event not found in events.rs".to_string(),
            };
        }

        // 5. Verify IriBuilder has inference_rule method
        let iri_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/iri.rs");
        let iri_content = match std::fs::read_to_string(&iri_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", iri_path.display(), e),
                };
            }
        };

        if !iri_content.contains("fn inference_rule") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "IriBuilder missing inference_rule method".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
