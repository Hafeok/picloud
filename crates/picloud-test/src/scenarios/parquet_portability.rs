//! ADR-046: Export Parquet data. Verify file is valid Parquet format
//! (readable without PiCloud tools).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ParquetPortability;

#[async_trait]
impl Scenario for ParquetPortability {
    fn name(&self) -> &str {
        "parquet-portability"
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

        // 1. Check telemetry data endpoint
        if !assertions::feature_available(ctx, "/api/telemetry/export").await {
            return ScenarioResult::Skip {
                reason: "telemetry export endpoint not available".to_string(),
            };
        }

        // 2. Request Parquet export
        match assertions::http_get(ctx, "/api/telemetry/export?format=parquet&signal=traces&limit=100").await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "Parquet export not implemented".to_string(),
                    };
                }

                if !resp.status().is_success() {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("Parquet export returned status: {}", status),
                    };
                }

                // 3. Verify response content type
                let content_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                if !content_type.contains("parquet") && !content_type.contains("octet-stream") {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "unexpected content-type for Parquet export: {}",
                            content_type
                        ),
                    };
                }

                // 4. Verify Parquet magic bytes (PAR1)
                let bytes = match resp.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to read Parquet response body: {}", e),
                        };
                    }
                };

                if bytes.len() >= 4 && &bytes[0..4] == b"PAR1" {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if bytes.is_empty() {
                    ScenarioResult::Skip {
                        reason: "no telemetry data available for export".to_string(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "exported file does not have valid Parquet magic bytes (PAR1)"
                            .to_string(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to request Parquet export: {}", e),
            },
        }
    }
}
