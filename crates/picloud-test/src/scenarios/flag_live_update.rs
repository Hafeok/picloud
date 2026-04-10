//! ADR-044: Verify feature flag resource types exist with live update capability.
//!
//! Reads domain source to confirm that FeatureFlag resource type exists,
//! FeatureFlagChanged event exists for live updates, and the version
//! expression evaluation logic is present.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct FlagLiveUpdate;

#[async_trait]
impl Scenario for FlagLiveUpdate {
    fn name(&self) -> &str {
        "flag-live-update"
    }

    fn adr(&self) -> &str {
        "ADR-044"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify FeatureFlag resource type exists
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

        if !resources_content.contains("struct FeatureFlag") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag resource type not found in resources.rs".to_string(),
            };
        }

        // 2. Verify FeatureFlag has enabled and version_expr fields
        if !resources_content.contains("pub enabled: bool") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag missing 'enabled' field".to_string(),
            };
        }

        if !resources_content.contains("pub version_expr: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag missing 'version_expr' field".to_string(),
            };
        }

        // 3. Verify is_active method exists for flag evaluation
        if !resources_content.contains("fn is_active") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlag missing is_active method for evaluation".to_string(),
            };
        }

        // 4. Verify VersionOp enum exists for expression evaluation
        if !resources_content.contains("enum VersionOp") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "VersionOp enum not found — needed for version expression evaluation"
                    .to_string(),
            };
        }

        // 5. Verify FeatureFlagChanged event for live updates
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

        if !events_content.contains("FeatureFlagChanged") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "FeatureFlagChanged event not found — live update mechanism missing".to_string(),
            };
        }

        if !events_content.contains("struct FeatureFlagChangedPayload") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "FeatureFlagChangedPayload struct not found".to_string(),
            };
        }

        // 6. Verify IriBuilder has feature_flag method
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

        if !iri_content.contains("fn feature_flag") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "IriBuilder missing feature_flag method".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
