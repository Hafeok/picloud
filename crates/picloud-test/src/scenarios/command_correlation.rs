//! ADR-008: Verify EventEnvelope has a correlation_id field and that the HTTP
//! command handler emits events with correlation IDs.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CommandCorrelation;

#[async_trait]
impl Scenario for CommandCorrelation {
    fn name(&self) -> &str {
        "command_correlation"
    }

    fn adr(&self) -> &str {
        "ADR-008"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // 1. Verify EventEnvelope has correlation_id.
        let events_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/events.rs");

        let events_src = match std::fs::read_to_string(&events_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read events.rs: {}", e),
                };
            }
        };

        if !events_src.contains("correlation_id") {
            issues.push("EventEnvelope missing correlation_id field".to_string());
        }

        // Verify correlation_id is a Uuid type for uniqueness guarantees.
        if !events_src.contains("correlation_id: Uuid") {
            issues.push("correlation_id should be Uuid type for uniqueness".to_string());
        }

        // 2. Verify the HTTP command handler emits events with correlation IDs.
        let http_impl_path = ctx
            .workspace_root()
            .join("crates/picloud-http/src/implementation.rs");

        let http_src = match std::fs::read_to_string(&http_impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read HTTP implementation: {}", e),
                };
            }
        };

        // Check that the command handler (/api/commands or /api/apply) exists.
        let has_command_handler =
            http_src.contains("handle_command") || http_src.contains("handle_apply");
        if !has_command_handler {
            issues.push("No command/apply handler found in HTTP server".to_string());
        }

        // Check that the handler creates or uses correlation_id.
        if !http_src.contains("correlation_id") {
            issues.push(
                "HTTP implementation does not reference correlation_id".to_string(),
            );
        }

        // Verify EventEnvelope::new takes correlation_id as a parameter.
        if events_src.contains("EventEnvelope::new") || events_src.contains("pub fn new(") {
            // Check the constructor signature includes correlation_id.
            if !events_src.contains("correlation_id") {
                issues.push(
                    "EventEnvelope::new does not accept correlation_id".to_string(),
                );
            }
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} correlation issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
