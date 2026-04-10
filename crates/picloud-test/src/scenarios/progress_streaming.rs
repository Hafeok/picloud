//! ADR-008: Check HTTP source for SSE streaming endpoint. Verify an event
//! stream endpoint exists for progress streaming.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ProgressStreaming;

#[async_trait]
impl Scenario for ProgressStreaming {
    fn name(&self) -> &str {
        "progress_streaming"
    }

    fn adr(&self) -> &str {
        "ADR-008"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // Read the HTTP implementation source.
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

        // Check for SSE (Server-Sent Events) support.
        let has_sse_import = http_src.contains("sse::")
            || http_src.contains("Sse")
            || http_src.contains("server_sent_events");

        if !has_sse_import {
            issues.push("No SSE support found in HTTP implementation".to_string());
        }

        // Check for an event stream endpoint.
        let has_events_route = http_src.contains("/events")
            || http_src.contains("handle_events")
            || http_src.contains("event_stream");

        if !has_events_route {
            issues.push(
                "No event stream endpoint found (expected /events or /api/events route)"
                    .to_string(),
            );
        }

        // Check for SSE KeepAlive (important for long-lived connections).
        if http_src.contains("Sse") && !http_src.contains("KeepAlive") {
            issues.push(
                "SSE endpoint exists but no KeepAlive configured — connections may drop"
                    .to_string(),
            );
        }

        // Check for correlation_id filtering in the event stream.
        // This is important for ADR-008: CLI subscribes filtered by correlation_id.
        let has_correlation_filter = http_src.contains("correlation_id")
            && (http_src.contains("filter") || http_src.contains("correlation"));

        if !has_correlation_filter {
            issues.push(
                "Event stream does not appear to support correlation_id filtering".to_string(),
            );
        }

        // Verify the Event type is used for SSE messages.
        if http_src.contains("Sse") && !http_src.contains("Event::") {
            // Event might be imported differently.
            if !http_src.contains("sse::Event") {
                issues.push(
                    "SSE endpoint does not use typed Event messages".to_string(),
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
                    "{} streaming issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
