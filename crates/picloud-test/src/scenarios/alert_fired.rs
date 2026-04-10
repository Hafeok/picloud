//! ADR-041: Verify AlertFired event type exists with correct fields.
//!
//! Reads domain events source to confirm that the AlertFired event variant
//! and AlertFiredPayload exist with the required fields: alert_type,
//! severity, message, resource_iri, rule_iri, fired_at.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AlertFired;

#[async_trait]
impl Scenario for AlertFired {
    fn name(&self) -> &str {
        "alert-fired"
    }

    fn adr(&self) -> &str {
        "ADR-041"
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

        // 1. Verify AlertFired variant in PlatformEvent
        if !events_content.contains("AlertFired(AlertFiredPayload)") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "AlertFired variant not found in PlatformEvent enum".to_string(),
            };
        }

        // 2. Verify AlertFiredPayload struct exists
        if !events_content.contains("struct AlertFiredPayload") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "AlertFiredPayload struct not found".to_string(),
            };
        }

        // 3. Verify required fields on AlertFiredPayload
        let required_fields = [
            "alert_type",
            "severity",
            "message",
            "resource_iri",
            "rule_iri",
            "fired_at",
        ];
        for field in &required_fields {
            if !events_content.contains(field) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("AlertFiredPayload missing field: {}", field),
                };
            }
        }

        // 4. Verify AlertSeverity enum exists
        if !events_content.contains("enum AlertSeverity") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "AlertSeverity enum not found".to_string(),
            };
        }

        // 5. Verify severity levels
        let severity_levels = ["Info", "Warning", "Critical"];
        for level in &severity_levels {
            if !events_content.contains(level) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("AlertSeverity missing level: {}", level),
                };
            }
        }

        // 6. Verify AlertResolved also exists (complete alert lifecycle)
        if !events_content.contains("AlertResolved(AlertResolvedPayload)") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "AlertResolved variant not found — alert lifecycle incomplete".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
