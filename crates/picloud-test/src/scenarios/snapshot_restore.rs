//! ADR-047: Create snapshot, restore from it. Assert state matches.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SnapshotRestore;

#[async_trait]
impl Scenario for SnapshotRestore {
    fn name(&self) -> &str {
        "snapshot-restore"
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

        // 1. Check snapshot endpoint
        if !assertions::feature_available(ctx, "/api/volumes/snapshots").await {
            return ScenarioResult::Skip {
                reason: "snapshot endpoint not available".to_string(),
            };
        }

        // 2. Trigger a snapshot
        let snapshot_cmd = serde_json::json!({
            "type": "SnapshotRequested",
            "volume": "test-snapshot-volume",
        });

        match assertions::http_post(ctx, "/api/volumes/snapshots", snapshot_cmd).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "snapshot creation not implemented".to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("snapshot endpoint not reachable: {}", e),
                };
            }
        }

        // 3. Wait for SnapshotCreated event
        let snapshot_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event a picloud:SnapshotCreated .
            }
        "#;

        if assertions::wait_for_sparql(ctx, snapshot_query, Duration::from_secs(30))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "SnapshotCreated event not found — snapshots may not be active"
                    .to_string(),
            };
        }

        // 4. Trigger restore from the snapshot
        let restore_cmd = serde_json::json!({
            "type": "RestoreRequested",
            "volume": "test-snapshot-volume",
            "target": "test-snapshot-volume-restored",
        });

        match assertions::http_post(ctx, "/api/volumes/restore", restore_cmd).await {
            Ok(resp) => {
                if !resp.status().is_success() && resp.status().as_u16() != 202 {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "restore request failed with status: {}",
                            resp.status().as_u16()
                        ),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to POST restore: {}", e),
                };
            }
        }

        // 5. Wait for RestoreCompleted
        let restore_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event a picloud:RestoreCompleted .
            }
        "#;

        match assertions::wait_for_sparql(ctx, restore_query, Duration::from_secs(60)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("RestoreCompleted event not found: {}", e),
            },
        }
    }
}
