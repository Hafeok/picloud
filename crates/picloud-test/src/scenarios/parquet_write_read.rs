//! ADR-046: Parquet write/read — query telemetry data and assert structured response.
//!
//! Query telemetry data via the DataFusion SQL endpoint. Assert the response
//! includes structured data (columns, rows, or a valid result set).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ParquetWriteRead;

#[async_trait]
impl Scenario for ParquetWriteRead {
    fn name(&self) -> &str {
        "parquet-write-read"
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

        // POST a SQL query to the DataFusion endpoint.
        let sql_body = serde_json::json!({
            "sql": "SELECT COUNT(*) AS total FROM traces",
            "signal": "traces"
        });

        let resp = match assertions::http_post(ctx, query_path, sql_body).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST telemetry SQL query failed: {}", e),
                };
            }
        };

        if !resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "telemetry SQL query endpoint returned status {}",
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

        if body.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "telemetry SQL query returned empty response".to_string(),
            };
        }

        // Verify the response is structured data (JSON with results).
        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "telemetry query response is not valid JSON: {}",
                        e
                    ),
                };
            }
        };

        // The response should contain columns, rows, data, or results.
        let has_structure = json.get("columns").is_some()
            || json.get("rows").is_some()
            || json.get("data").is_some()
            || json.get("results").is_some()
            || json.as_array().is_some();

        if !has_structure {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "telemetry query response lacks structured data fields".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
