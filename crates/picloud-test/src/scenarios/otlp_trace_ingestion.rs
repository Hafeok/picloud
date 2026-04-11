//! ADR-045: OTLP trace ingestion — POST OTLP data to the telemetry endpoint.
//!
//! POST OTLP data to /api/telemetry (or /otel/v1/traces). Assert 200 or 204.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OtlpTraceIngestion;

#[async_trait]
impl Scenario for OtlpTraceIngestion {
    fn name(&self) -> &str {
        "otlp-trace-ingestion"
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

        // Try the OTLP traces endpoint.
        let otlp_path = "/otel";
        if !assertions::feature_available(ctx, otlp_path).await {
            return ScenarioResult::Skip {
                reason: "OTLP traces endpoint not implemented yet".to_string(),
            };
        }

        // POST a minimal OTLP JSON trace payload.
        let otlp_payload = serde_json::json!({
            "resourceSpans": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": { "stringValue": "picloud-test" }
                    }]
                },
                "scopeSpans": [{
                    "scope": { "name": "picloud-test" },
                    "spans": [{
                        "traceId": "00000000000000000000000000000001",
                        "spanId": "0000000000000001",
                        "name": "test-span",
                        "kind": 1,
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano": "1700000001000000000",
                        "status": { "code": 1 }
                    }]
                }]
            }]
        });

        let resp = match assertions::http_post(ctx, otlp_path, otlp_payload).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST OTLP trace failed: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();
        if status != 200 && status != 204 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "OTLP trace ingestion returned status {} — expected 200 or 204",
                    status
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
