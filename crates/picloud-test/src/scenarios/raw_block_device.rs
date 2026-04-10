//! ADR-012: Raw block device — apply raw block device volume, verify block device
//! node exists.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RawBlockDevice;

#[async_trait]
impl Scenario for RawBlockDevice {
    fn name(&self) -> &str {
        "raw-block-device"
    }

    fn adr(&self) -> &str {
        "ADR-012"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/volumes").await {
            return ScenarioResult::Skip {
                reason: "volume API not available".to_string(),
            };
        }

        // Apply a raw block device volume resource.
        let volume_body = serde_json::json!({
            "type": "volume",
            "name": "test-raw-block",
            "product": "picloud-test",
            "spec": {
                "mode": "raw",
                "size": "1Gi",
                "durability": "full-replication"
            }
        });

        match assertions::http_post(ctx, "/api/resources", volume_body).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!("raw block device resource applied");
            }
            Ok(resp) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("raw block device apply returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("raw block device apply failed: {}", e),
                };
            }
        }

        // Wait for volume to be ready in RDF graph.
        let ready_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                <https://picloud.local/products/picloud-test/volumes/test-raw-block>
                    picloud:status picloud:Ready .
            }
        "#;

        if let Err(e) =
            assertions::wait_for_sparql(ctx, ready_query, Duration::from_secs(60)).await
        {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("raw block device did not reach Ready state: {}", e),
            };
        }

        // Verify the block device node is recorded in the RDF graph.
        let device_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?device WHERE {
                <https://picloud.local/products/picloud-test/volumes/test-raw-block>
                    picloud:blockDevice ?device .
            }
        "#;

        match assertions::sparql_query(ctx, device_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse device query response: {}", e),
                        };
                    }
                };

                let bindings = json
                    .pointer("/results/bindings")
                    .and_then(|v| v.as_array());

                match bindings {
                    Some(arr) if !arr.is_empty() => {
                        let device = arr[0]
                            .pointer("/device/value")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        info!(device = %device, "raw block device node confirmed");
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    }
                    _ => ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "raw block device has no blockDevice property in RDF graph"
                            .to_string(),
                    },
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("device query failed: {}", e),
            },
        }
    }
}
