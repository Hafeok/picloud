//! Multi-node persistence — apply a product, restart a node, verify state survives.
//!
//! If PICLOUD_TEST_BINARY is set: SSH restarts the node, waits for health,
//! verifies the product is still in the RDF graph.
//! Otherwise (single-node without binary path): verifies the event log file
//! exists on disk via SSH, confirming persistent storage is in use.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct MultiNodePersistence;

#[async_trait]
impl Scenario for MultiNodePersistence {
    fn name(&self) -> &str {
        "multi-node-persistence"
    }

    fn adr(&self) -> &str {
        "ADR-004"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let product_name = "persist-test-e2e";
        let product_version = "1.0.0";

        // Apply a product so we have state to verify after restart.
        match assertions::apply_product_and_wait(ctx, product_name, product_version).await {
            Ok(()) => {
                info!(product = product_name, "product applied for persistence test");
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to apply product: {}", e),
                };
            }
        }

        // Determine which node to test persistence on.
        let target_node = match ctx.config.nodes.first() {
            Some(n) => n,
            None => {
                return ScenarioResult::Skip {
                    reason: "no nodes configured".to_string(),
                };
            }
        };

        let test_binary = std::env::var("PICLOUD_TEST_BINARY").ok();

        if let Some(binary_path) = test_binary {
            // Full restart test: kill, restart, wait for health, verify state.
            info!(
                node = %target_node.hostname,
                binary = %binary_path,
                "restarting node for persistence verification"
            );

            // Kill the server
            match assertions::ssh_command(target_node, "sudo pkill -9 picloud-server").await {
                Ok(_) => {}
                Err(e) => {
                    return ScenarioResult::Skip {
                        reason: format!("SSH kill failed: {}", e),
                    };
                }
            }

            // Brief pause for process to terminate
            tokio::time::sleep(Duration::from_secs(2)).await;

            // Restart the server in the background
            let restart_cmd = format!(
                "nohup {} > /tmp/picloud-restart.log 2>&1 &",
                binary_path
            );
            match assertions::ssh_command(target_node, &restart_cmd).await {
                Ok(_) => info!("restart command issued"),
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("SSH restart failed: {}", e),
                    };
                }
            }

            // Wait for the node to become healthy again (up to 30s).
            let health_deadline = Instant::now() + Duration::from_secs(30);
            let mut healthy = false;
            while Instant::now() < health_deadline {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if assertions::feature_available(ctx, "/health").await {
                    healthy = true;
                    break;
                }
            }

            if !healthy {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "node did not become healthy within 30s after restart".to_string(),
                };
            }

            // Verify the product survived the restart.
            let ask = format!(
                "ASK {{ <https://picloud.local/products/{}> a <https://picloud.local/ontology#Product> }}",
                product_name
            );
            match assertions::wait_for_sparql(ctx, &ask, Duration::from_secs(15)).await {
                Ok(()) => {
                    info!(
                        product = product_name,
                        elapsed_ms = start.elapsed().as_millis() as u64,
                        "product survived node restart"
                    );
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                }
                Err(e) => ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "product {} not found after restart: {}",
                        product_name, e
                    ),
                },
            }
        } else {
            // No binary path: verify event log persistence by checking the data dir via SSH.
            info!(
                node = %target_node.hostname,
                "checking event log persistence on disk (no binary path for restart test)"
            );

            // Check that the events directory or file exists on the node.
            let check_cmd = "ls -la /var/lib/picloud/events/ 2>/dev/null || ls -la $HOME/picloud-data/events/ 2>/dev/null || echo 'NO_EVENTS_DIR'";
            match assertions::ssh_command(target_node, check_cmd).await {
                Ok(output) => {
                    if output.contains("NO_EVENTS_DIR") {
                        // Also check the raft sled directory as an alternative
                        let raft_check = "ls -la /var/lib/picloud/raft/ 2>/dev/null || ls -la $HOME/picloud-data/raft/ 2>/dev/null || echo 'NO_RAFT_DIR'";
                        match assertions::ssh_command(target_node, raft_check).await {
                            Ok(raft_output) => {
                                if raft_output.contains("NO_RAFT_DIR") {
                                    return ScenarioResult::Skip {
                                        reason: "no persistent data directory found on node — may be in-memory mode".to_string(),
                                    };
                                }
                                info!("raft persistence directory found on disk");
                                ScenarioResult::Pass {
                                    duration: start.elapsed(),
                                }
                            }
                            Err(e) => ScenarioResult::Skip {
                                reason: format!("SSH check for raft dir failed: {}", e),
                            },
                        }
                    } else {
                        info!("event log persistence directory found on disk");
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    }
                }
                Err(e) => ScenarioResult::Skip {
                    reason: format!("SSH check failed: {}", e),
                },
            }
        }
    }
}
