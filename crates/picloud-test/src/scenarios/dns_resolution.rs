//! ADR-003: mDNS for Node Discovery — verify picloud.local resolves to a cluster node.
//!
//! Checks that the picloud-server binary exists and runs with --help. Then attempts
//! DNS resolution of picloud.local. Skips if no cluster is available.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct DnsResolutionScenario;

#[async_trait]
impl Scenario for DnsResolutionScenario {
    fn name(&self) -> &str {
        "dns-resolution"
    }

    fn adr(&self) -> &str {
        "ADR-003"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Locate picloud-server binary.
        let server_bin = ctx.workspace_root().join("target/release/picloud-server");
        if !server_bin.exists() {
            return ScenarioResult::Skip {
                reason: format!(
                    "picloud-server binary not found at {}",
                    server_bin.display()
                ),
            };
        }

        // Verify the binary runs (--help / --version).
        let output = match tokio::process::Command::new(&server_bin)
            .arg("--help")
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to execute picloud-server --help: {}", e),
                };
            }
        };

        if !output.status.success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "picloud-server --help exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                ),
            };
        }

        info!("picloud-server binary runs successfully");

        // Attempt DNS resolution of picloud.local via system resolver.
        let resolve_output = tokio::process::Command::new("getent")
            .args(["hosts", "picloud.local"])
            .output()
            .await;

        match resolve_output {
            Ok(res) if res.status.success() => {
                let stdout = String::from_utf8_lossy(&res.stdout);
                let resolved_ip = stdout.split_whitespace().next().unwrap_or("unknown");
                info!(ip = resolved_ip, "picloud.local resolved successfully");

                // Verify the resolved IP matches a configured node if we have node config.
                if !ctx.config.nodes.is_empty() {
                    let node_ips: Vec<&str> =
                        ctx.config.nodes.iter().map(|n| n.ip.as_str()).collect();
                    if !node_ips.contains(&resolved_ip) {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "picloud.local resolved to {} which is not a configured node IP ({:?})",
                                resolved_ip, node_ips
                            ),
                        };
                    }
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Ok(_) => {
                // Resolution failed — skip if no cluster nodes configured (offline test).
                if ctx.config.nodes.is_empty() {
                    return ScenarioResult::Skip {
                        reason: "picloud.local does not resolve and no cluster nodes configured — no cluster available".to_string(),
                    };
                }
                ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "picloud.local does not resolve but cluster nodes are configured".to_string(),
                }
            }
            Err(e) => {
                if ctx.config.nodes.is_empty() {
                    return ScenarioResult::Skip {
                        reason: format!(
                            "DNS lookup tool unavailable and no cluster configured: {}",
                            e
                        ),
                    };
                }
                ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to run DNS resolution: {}", e),
                }
            }
        }
    }
}
