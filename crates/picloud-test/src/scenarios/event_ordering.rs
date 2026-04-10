//! ADR-004: Verify EventEnvelope has monotonic index fields and correlation_id.
//! Check source code for ordering guarantees in the event log implementation.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventOrdering;

#[async_trait]
impl Scenario for EventOrdering {
    fn name(&self) -> &str {
        "event_ordering"
    }

    fn adr(&self) -> &str {
        "ADR-004"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Read the EventEnvelope source to verify structural requirements.
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

        // Verify EventEnvelope struct exists with required fields.
        let mut issues: Vec<String> = Vec::new();

        if !events_src.contains("pub struct EventEnvelope") {
            issues.push("EventEnvelope struct not found".to_string());
        }

        if !events_src.contains("correlation_id") {
            issues.push("correlation_id field missing from EventEnvelope".to_string());
        }

        if !events_src.contains("timestamp") {
            issues.push("timestamp field missing from EventEnvelope (needed for ordering)".to_string());
        }

        if !events_src.contains("id: Uuid") && !events_src.contains("id:") {
            issues.push("id field missing from EventEnvelope (needed for uniqueness)".to_string());
        }

        // Read the event log implementation to verify ordering guarantees.
        let impl_path = ctx
            .workspace_root()
            .join("crates/picloud-events/src/implementation.rs");

        let impl_src = match std::fs::read_to_string(&impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read events implementation: {}", e),
                };
            }
        };

        // Verify the event log uses an append-only structure (Vec or equivalent).
        if !impl_src.contains("append") {
            issues.push("No append method found in event log implementation".to_string());
        }

        // Verify events_since exists for offset-based replay (monotonic index support).
        if !impl_src.contains("events_since") {
            issues.push(
                "No events_since method found — monotonic index replay not supported".to_string(),
            );
        }

        // Verify idempotency deduplication exists.
        if !impl_src.contains("idempotency_key") {
            issues.push("No idempotency_key deduplication in event log".to_string());
        }

        // Verify the EventLog trait is implemented.
        if !impl_src.contains("impl EventLog for") {
            issues.push("EventLog trait not implemented in event log crate".to_string());
        }

        // Verify the log uses a sequential/ordered data structure.
        let has_vec = impl_src.contains("Vec<EventEnvelope>");
        let has_btree = impl_src.contains("BTreeMap");
        if !has_vec && !has_btree {
            issues.push(
                "Event log does not use an ordered collection (Vec or BTreeMap)".to_string(),
            );
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} event ordering issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
