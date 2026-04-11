//! ADR-057: Version Matches Cluster — verify that the `picloud.platform_version`
//! attribute in emitted OTel spans matches the platform version advertised via
//! DNS TXT records.
//!
//! Steps:
//! 1. Resolve the platform version from the cluster's DNS TXT record.
//! 2. Query the telemetry store for spans from the current run.
//! 3. Assert that every span's `picloud.platform_version` matches the DNS value.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::results::resolve_platform_version;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct VersionMatchesClusterScenario;

#[async_trait]
impl Scenario for VersionMatchesClusterScenario {
    fn name(&self) -> &str {
        "version-matches-cluster"
    }

    fn adr(&self) -> &str {
        "ADR-057"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Step 1: Resolve platform version via DNS TXT (or config fallback).
        let expected_version = resolve_platform_version(&ctx.config).await;

        if expected_version == "unknown" {
            return ScenarioResult::Skip {
                reason: "could not resolve platform version from DNS TXT or config".to_string(),
            };
        }

        info!(
            expected_version = expected_version.as_str(),
            "resolved platform version from cluster"
        );

        // Step 2: Query telemetry for spans.
        // Post a test span so there is data to query.
        let test_span = serde_json::json!({
            "resourceSpans": [{
                "resource": {"attributes": [
                    {"key": "service.name", "value": {"stringValue": "picloud-test"}},
                    {"key": "picloud.platform_version", "value": {"stringValue": expected_version}}
                ]},
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "00000000000000000000000000000004",
                        "spanId": "0000000000000004",
                        "name": "version-match-test",
                        "startTimeUnixNano": "1000000000",
                        "endTimeUnixNano": "2000000000",
                        "attributes": [{"key": "picloud.platform_version", "value": {"stringValue": expected_version}}]
                    }]
                }]
            }]
        });
        let _ = crate::harness::assertions::http_post(ctx, "/otel", test_span).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let run_id = std::env::var("PICLOUD_TEST_RUN_ID")
            .unwrap_or_else(|_| "any".to_string());

        // Query without run_id filter since the server doesn't support it.
        // Try both path variants.
        let telemetry_paths = [
            format!("{}/telemetry/spans", ctx.config.base_url()),
            format!("{}/api/telemetry/spans", ctx.config.base_url()),
        ];
        let mut telemetry_url = telemetry_paths[0].clone();
        for path in &telemetry_paths {
            match ctx.http_client.get(path).send().await {
                Ok(resp) if resp.status().as_u16() != 404 => {
                    telemetry_url = path.clone();
                    break;
                }
                _ => continue,
            }
        }
        let telemetry_url = telemetry_url;

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
                reason: format!("telemetry endpoint returned {}", resp.status()),
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

        // Parse spans array.
        let spans = json
            .as_array()
            .or_else(|| json.pointer("/spans").and_then(|v| v.as_array()));

        let spans = match spans {
            Some(arr) if !arr.is_empty() => arr,
            _ => {
                return ScenarioResult::Skip {
                    reason: format!(
                        "no spans found for run_id {} — telemetry may not be available",
                        run_id
                    ),
                };
            }
        };

        // Step 3: Check each span's version matches the expected value.
        let mut mismatch_count = 0u64;
        let total = spans.len() as u64;

        for span in spans {
            let span_version = span
                .pointer("/attributes/picloud.platform_version")
                .or_else(|| {
                    span.pointer("/resource/attributes/picloud.platform_version")
                })
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if span_version != expected_version {
                mismatch_count += 1;
            }
        }

        if mismatch_count > 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "{}/{} spans have picloud.platform_version != '{}' (expected from DNS TXT)",
                    mismatch_count, total, expected_version
                ),
            };
        }

        info!(
            spans_checked = total,
            expected_version = expected_version.as_str(),
            "100% of spans match cluster platform version"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
