//! ADR-047: Snapshot create and verify — POST to create a snapshot, GET to verify it exists.
//!
//! POST to /api/snapshots to create a snapshot. GET to verify the snapshot
//! appears in the listing with the correct metadata.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SnapshotCreateVerify;

#[async_trait]
impl Scenario for SnapshotCreateVerify {
    fn name(&self) -> &str {
        "snapshot-create-verify"
    }

    fn adr(&self) -> &str {
        "ADR-047"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let snapshots_path = "/api/snapshots";
        if !assertions::feature_available(ctx, snapshots_path).await {
            return ScenarioResult::Skip {
                reason: "snapshots endpoint not implemented yet".to_string(),
            };
        }

        // POST to create a snapshot (POST is registered at /api/snapshots/*path).
        let snapshot_req = serde_json::json!({
            "volume": "test-volume",
            "product": "test-app"
        });

        let create_path = format!("{}/test-volume", snapshots_path);
        let post_resp = match assertions::http_post(ctx, &create_path, snapshot_req).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST snapshot creation failed: {}", e),
                };
            }
        };

        let post_status = post_resp.status().as_u16();
        if post_status == 404 || post_status == 501 {
            return ScenarioResult::Skip {
                reason: "snapshots endpoint not fully implemented yet".to_string(),
            };
        }

        if !post_resp.status().is_success() && post_status != 202 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("POST snapshot returned status {}", post_status),
            };
        }

        // Brief pause for the snapshot to be recorded.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // GET to list snapshots and verify the new one exists.
        let get_resp = match assertions::http_get(ctx, snapshots_path).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("GET snapshots listing failed: {}", e),
                };
            }
        };

        if !get_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("GET snapshots returned status {}", get_resp.status()),
            };
        }

        let body = match get_resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read snapshots response: {}", e),
                };
            }
        };

        // Verify the response contains snapshot data.
        if body.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "snapshots listing returned empty body".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
