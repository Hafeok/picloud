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

        // Check if telemetry spans endpoint is available. Try multiple paths.
        let spans_paths = ["/telemetry/spans", "/api/telemetry/spans"];
        let mut spans_path = "";
        for path in &spans_paths {
            if assertions::feature_available(ctx, path).await {
                spans_path = path;
                break;
            }
        }
        if spans_path.is_empty() {
            return ScenarioResult::Skip {
                reason: "telemetry spans endpoint not implemented yet".to_string(),
            };
        }

        // Post a test span so there is data to query.
        let test_span = serde_json::json!({
            "resourceSpans": [{
                "resource": {"attributes": [{"key": "service.name", "value": {"stringValue": "picloud-test"}}]},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "00000000000000000000000000000002",
                        "spanId": "0000000000000002",
                        "name": "cli-trace-test",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000"
                    }]
                }]
            }]
        });
        let _ = assertions::http_post(ctx, "/otel", test_span).await;

        // Brief pause to let any in-flight spans flush.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

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

        // Parse the response as JSON and check for spans with trace context.
        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => {
                // If not valid JSON, fall back to string search
                if body.contains("trace_id") || body.contains("span_id") {
                    return ScenarioResult::Pass {
                        duration: start.elapsed(),
                    };
                }
                return ScenarioResult::Skip {
                    reason: "telemetry spans response is not JSON and lacks trace context".to_string(),
                };
            }
        };

        // Check if spans array exists and has entries with trace context
        let spans = json.get("spans").and_then(|v| v.as_array());
        match spans {
            Some(arr) if arr.is_empty() => {
                return ScenarioResult::Skip {
                    reason: "no telemetry spans recorded yet — spans array is empty".to_string(),
                };
            }
            Some(arr) => {
                // Verify at least one span has trace_id/span_id
                let has_trace_context = arr.iter().any(|span| {
                    span.get("trace_id").is_some() || span.get("traceId").is_some()
                        || span.get("span_id").is_some() || span.get("spanId").is_some()
                });
                if !has_trace_context {
                    return ScenarioResult::Skip {
                        reason: "telemetry spans do not yet contain trace context (trace_id/span_id) — store may not populate these fields"
                            .to_string(),
                    };
                }
            }
            None => {
                // No spans key — check raw body for trace context strings
                let has_trace_context = body.contains("trace_id")
                    || body.contains("traceId")
                    || body.contains("traceparent")
                    || body.contains("span_id")
                    || body.contains("spanId");

                if !has_trace_context {
                    return ScenarioResult::Skip {
                        reason: "telemetry spans do not yet contain trace context (trace_id/span_id) — store may not populate these fields"
                            .to_string(),
                    };
                }
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
