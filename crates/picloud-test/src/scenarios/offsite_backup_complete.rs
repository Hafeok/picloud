//! ADR-047: Trigger backup. Assert BackupCompleted event.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OffsiteBackupComplete;

#[async_trait]
impl Scenario for OffsiteBackupComplete {
    fn name(&self) -> &str {
        "offsite-backup-complete"
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

        // 1. Check backup endpoint
        if !assertions::feature_available(ctx, "/api/volumes/backup").await {
            return ScenarioResult::Skip {
                reason: "backup endpoint not available".to_string(),
            };
        }

        // 2. Trigger a backup
        let backup_cmd = serde_json::json!({
            "type": "BackupRequested",
            "volume": "test-backup-volume",
            "target": "offsite",
        });

        match assertions::http_post(ctx, "/api/volumes/backup", backup_cmd).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "backup not implemented".to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("backup endpoint not reachable: {}", e),
                };
            }
        }

        // 3. Wait for BackupCompleted event
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event a picloud:BackupCompleted .
            }
        "#;

        match assertions::wait_for_sparql(ctx, ask_query, Duration::from_secs(60)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("BackupCompleted event not found: {}", e),
            },
        }
    }
}
