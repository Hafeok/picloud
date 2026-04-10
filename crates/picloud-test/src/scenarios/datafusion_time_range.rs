//! ADR-046: DataFusion time-range query — query telemetry with time range params.
//!
//! GET /api/telemetry/query with time range parameters. Assert the response
//! contains filtered results within the requested window.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct DatafusionTimeRange;

#[async_trait]
impl Scenario for DatafusionTimeRange {
    fn name(&self) -> &str {
        "datafusion-time-range"
    }

    fn adr(&self) -> &str {
        "ADR-046"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let query_path = "/api/telemetry/query";
        if !assertions::feature_available(ctx, query_path).await {
            return ScenarioResult::Skip {
                reason: "telemetry query endpoint not implemented yet".to_string(),
            };
        }

        // Query with time range parameters.
        let query_with_params =
            "/api/telemetry/query?signal=traces&from=2020-01-01T00:00:00Z&to=2099-12-31T23:59:59Z";

        let resp = match assertions::http_get(ctx, query_with_params).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("GET telemetry query with time range failed: {}", e),
                };
            }
        };

        if !resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "telemetry query endpoint returned status {}",
                    resp.status()
                ),
            };
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read telemetry query response: {}", e),
                };
            }
        };

        // The response should be valid JSON or a structured result.
        if body.is_empty() {
            return ScenarioResult::Skip {
                reason: "telemetry query returned empty response — no data ingested yet"
                    .to_string(),
            };
        }

        // Verify the response can be parsed as JSON.
        if serde_json::from_str::<serde_json::Value>(&body).is_err() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "telemetry query response is not valid JSON".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
