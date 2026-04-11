//! ADR-055: Rolling Upgrade Sequence — verify followers upgrade before leader
//! and that quorum is maintained throughout.
//!
//! Performs a rolling upgrade and validates:
//! - Followers are upgraded before the leader (from the upgrade report order).
//! - During the upgrade, SPARQL is polled every 1 second to confirm at least
//!   one node holds `picloud:hasRole picloud:Leader` at all times.
//! - Zero quorum gaps exceed 5 seconds.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};
use crate::upgrade::{rolling_upgrade, UpgradeConfig};

pub struct RollingUpgradeSequenceScenario;

const LEADER_QUERY: &str = r#"
    PREFIX picloud: <https://picloud.local/ontology#>
    SELECT ?node WHERE {
        ?node picloud:hasRole picloud:Leader .
    }
"#;

/// Check whether the SPARQL response contains at least one leader binding.
fn has_leader(body: &str) -> bool {
    let json: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    json.pointer("/results")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false)
}

#[async_trait]
impl Scenario for RollingUpgradeSequenceScenario {
    fn name(&self) -> &str {
        "rolling-upgrade-sequence"
    }

    fn adr(&self) -> &str {
        "ADR-055"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Check for the test binary env var.
        let binary_path = match std::env::var("PICLOUD_TEST_BINARY") {
            Ok(p) => PathBuf::from(p),
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "PICLOUD_TEST_BINARY not set".to_string(),
                };
            }
        };

        if !binary_path.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("binary not found at {}", binary_path.display()),
            };
        }

        // Shared state for the leader-polling background task.
        let upgrade_done = Arc::new(AtomicBool::new(false));
        let max_gap = Arc::new(std::sync::Mutex::new(Duration::ZERO));

        // Spawn a background task that polls for leader presence every second.
        let poll_done = Arc::clone(&upgrade_done);
        let poll_gap = Arc::clone(&max_gap);
        let poll_base_url = ctx.config.base_url();
        let poll_client = ctx.http_client.clone();

        let poll_handle = tokio::spawn(async move {
            let mut last_leader_seen = Instant::now();
            let mut gap_count = 0u64;

            while !poll_done.load(Ordering::Relaxed) {
                let url = format!("{}/graph", poll_base_url);
                let leader_present = match poll_client
                    .get(&url)
                    .query(&[("query", LEADER_QUERY)])
                    .header("Accept", "application/sparql-results+json")
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        match resp.text().await {
                            Ok(body) => has_leader(&body),
                            Err(_) => false,
                        }
                    }
                    _ => false,
                };

                if leader_present {
                    let gap = last_leader_seen.elapsed();
                    // Only record gap if we lost leadership for a measurable period.
                    if gap > Duration::from_secs(2) {
                        let mut mg = poll_gap.lock().unwrap();
                        if gap > *mg {
                            *mg = gap;
                        }
                        gap_count += 1;
                    }
                    last_leader_seen = Instant::now();
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            // Record final gap if leader was absent at the end.
            let final_gap = last_leader_seen.elapsed();
            if final_gap > Duration::from_secs(2) {
                let mut mg = poll_gap.lock().unwrap();
                if final_gap > *mg {
                    *mg = final_gap;
                }
            }

            gap_count
        });

        // Run the rolling upgrade.
        let upgrade_config = UpgradeConfig {
            binary_path,
            cluster_config: ctx.config.clone(),
        };

        let upgrade_result = rolling_upgrade(upgrade_config)
            .await
            .map_err(|e| format!("rolling upgrade error: {}", e));
        let report = match upgrade_result {
            Ok(r) => r,
            Err(reason) => {
                upgrade_done.store(true, Ordering::Relaxed);
                let _ = poll_handle.await;
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason,
                };
            }
        };

        // Signal the polling task to stop and collect results.
        upgrade_done.store(true, Ordering::Relaxed);
        let _gap_count = poll_handle.await.unwrap_or(0);

        if !report.success {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "rolling upgrade reported failure".to_string(),
            };
        }

        // Validate upgrade order: all followers must appear before the leader.
        let mut saw_leader = false;
        for node in &report.nodes {
            if node.role == "leader" {
                saw_leader = true;
            } else if saw_leader {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "follower {} was upgraded after the leader",
                        node.hostname
                    ),
                };
            }
        }

        info!(
            upgrade_order = ?report.nodes.iter().map(|n| format!("{}({})", n.hostname, n.role)).collect::<Vec<_>>(),
            "upgrade order validated: followers before leader"
        );

        // Check maximum quorum gap.
        let observed_gap = *max_gap.lock().unwrap();
        if observed_gap > Duration::from_secs(5) {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "quorum gap of {:.1}s exceeded 5s threshold",
                    observed_gap.as_secs_f64()
                ),
            };
        }

        info!(
            max_gap_ms = observed_gap.as_millis() as u64,
            "quorum maintained within threshold"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
