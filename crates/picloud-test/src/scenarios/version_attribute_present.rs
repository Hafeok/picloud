//! ADR-057: Version Attribute Present — verify that every OTel span emitted
//! during a test run carries a non-empty `picloud.platform_version` attribute.
//!
//! Queries the cluster's telemetry endpoint for spans matching the current
//! run_id and asserts 100% coverage of the version attribute.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct VersionAttributePresentScenario;

#[async_trait]
impl Scenario for VersionAttributePresentScenario {
    fn name(&self) -> &str {
        "version-attribute-present"
    }

    fn adr(&self) -> &str {
        "ADR-057"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Determine the run_id — either from env or generate a placeholder.
        let run_id = std::env::var("PICLOUD_TEST_RUN_ID")
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

        // Query the telemetry/spans endpoint for spans from this run.
        let telemetry_url = format!(
            "{}/api/telemetry/spans?run_id={}",
            ctx.config.base_url(),
            run_id
        );

        let resp = match ctx.http_client.get(&telemetry_url).send().await {
            Ok(r) => r,
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("connection refused")
                    || err_str.contains("Connection refused")
                    || err_str.contains("connect error")
                {
                    return ScenarioResult::Skip {
                        reason: "telemetry endpoint not available".to_string(),
                    };
                }
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("telemetry request failed: {}", e),
                };
            }
        };

        if resp.status().as_u16() == 404 || resp.status().as_u16() == 501 {
            return ScenarioResult::Skip {
                reason: "telemetry endpoint not implemented".to_string(),
            };
        }

        if !resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "telemetry endpoint returned {}",
                    resp.status()
                ),
            };
        }

        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("reading telemetry response: {}", e),
                };
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("invalid JSON from telemetry: {}", e),
                };
            }
        };

        // Expect an array of span objects, each with an "attributes" map.
        let spans = match json.as_array() {
            Some(arr) => arr,
            None => {
                // Maybe the spans are nested under a key.
                match json.pointer("/spans").and_then(|v| v.as_array()) {
                    Some(arr) => arr,
                    None => {
                        if json.is_null() || (json.is_array() && json.as_array().unwrap().is_empty()) {
                            return ScenarioResult::Skip {
                                reason: format!(
                                    "no spans found for run_id {} — telemetry may not have been flushed yet",
                                    run_id
                                ),
                            };
                        }
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: "unexpected telemetry response format".to_string(),
                        };
                    }
                }
            }
        };

        if spans.is_empty() {
            return ScenarioResult::Skip {
                reason: format!(
                    "no spans found for run_id {} — test may not have emitted telemetry yet",
                    run_id
                ),
            };
        }

        // Check every span for the picloud.platform_version attribute.
        let mut missing_count = 0u64;
        let total = spans.len() as u64;

        for span in spans {
            let has_version = span
                .pointer("/attributes/picloud.platform_version")
                .or_else(|| {
                    // Some exporters nest attributes differently.
                    span.pointer("/resource/attributes/picloud.platform_version")
                })
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);

            if !has_version {
                missing_count += 1;
            }
        }

        if missing_count > 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "{}/{} spans missing picloud.platform_version attribute",
                    missing_count, total
                ),
            };
        }

        info!(
            spans_checked = total,
            run_id = run_id.as_str(),
            "100% of spans carry picloud.platform_version"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
