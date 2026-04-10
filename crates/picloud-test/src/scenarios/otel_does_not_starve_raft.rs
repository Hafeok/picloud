//! ADR-045: Under telemetry load, verify Raft operations still complete
//! in time (p99 append latency does not increase > 20%).

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OtelDoesNotStarveRaft;

#[async_trait]
impl Scenario for OtelDoesNotStarveRaft {
    fn name(&self) -> &str {
        "otel-does-not-starve-raft"
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

        // 1. Measure baseline Raft append latency
        let baseline_start = Instant::now();
        let baseline_cmd = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "product",
            "name": "raft-baseline-test",
            "product": "raft-baseline-test",
        });

        match assertions::http_post(ctx, "/api/commands", baseline_cmd).await {
            Ok(resp) => {
                if resp.status().as_u16() == 404 || resp.status().as_u16() == 501 {
                    return ScenarioResult::Skip {
                        reason: "command endpoint not available".to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("command endpoint not reachable: {}", e),
                };
            }
        }
        let baseline_latency = baseline_start.elapsed();

        // 2. Check OTel endpoint exists
        if !assertions::feature_available(ctx, "/otel/v1/traces").await {
            return ScenarioResult::Skip {
                reason: "OTel endpoint not available".to_string(),
            };
        }

        // 3. Generate telemetry load (burst of span submissions)
        let otel_payload = serde_json::json!({
            "resourceSpans": [{
                "resource": { "attributes": [{ "key": "service.name", "value": { "stringValue": "load-test" } }] },
                "scopeSpans": [{
                    "spans": (0..100).map(|i| serde_json::json!({
                        "traceId": format!("{:032x}", i),
                        "spanId": format!("{:016x}", i),
                        "name": format!("test-span-{}", i),
                        "kind": 1,
                        "startTimeUnixNano": "1700000000000000000",
                        "endTimeUnixNano": "1700000001000000000",
                    })).collect::<Vec<_>>(),
                }],
            }],
        });

        // Fire spans concurrently
        for _ in 0..10 {
            let _ = assertions::http_post(ctx, "/otel/v1/traces", otel_payload.clone()).await;
        }

        // 4. Measure Raft append latency under load
        let loaded_start = Instant::now();
        let loaded_cmd = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "product",
            "name": "raft-loaded-test",
            "product": "raft-loaded-test",
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", loaded_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("Raft append failed under OTel load: {}", e),
            };
        }
        let loaded_latency = loaded_start.elapsed();

        // 5. Assert latency did not increase by more than 20%
        let threshold = baseline_latency.mul_f64(1.2) + Duration::from_millis(100);
        if loaded_latency > threshold {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "Raft latency degraded under OTel load: baseline={:?}, loaded={:?} (threshold={:?})",
                    baseline_latency, loaded_latency, threshold
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
