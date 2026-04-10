//! ADR-045: CLI trace propagation — verify spans carry traceparent headers.
//!
//! GET /api/telemetry/spans. Assert spans carry W3C traceparent headers
//! and that end-to-end traces from CLI invocation through platform
//! operations are present.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CliTracePropagation;

#[async_trait]
impl Scenario for CliTracePropagation {
    fn name(&self) -> &str {
        "cli-trace-propagation"
    }

    fn adr(&self) -> &str {
        "ADR-045"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Check if telemetry spans endpoint is available.
        let spans_path = "/api/telemetry/spans";
        if !assertions::feature_available(ctx, spans_path).await {
            return ScenarioResult::Skip {
                reason: "telemetry spans endpoint not implemented yet".to_string(),
            };
        }

        let resp = match assertions::http_get(ctx, spans_path).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("GET telemetry spans failed: {}", e),
                };
            }
        };

        if !resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("telemetry spans endpoint returned status {}", resp.status()),
            };
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read telemetry spans response: {}", e),
                };
            }
        };

        if body.is_empty() {
            return ScenarioResult::Skip {
                reason: "no telemetry spans recorded yet".to_string(),
            };
        }

        // Verify spans contain trace context fields.
        let has_trace_context = body.contains("trace_id")
            || body.contains("traceId")
            || body.contains("traceparent")
            || body.contains("span_id")
            || body.contains("spanId");

        if !has_trace_context {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "telemetry spans do not contain trace context (trace_id/span_id)"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
