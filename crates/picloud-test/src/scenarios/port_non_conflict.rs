//! ADR-056: Port Non-Conflict — verify production and staging systemd services
//! coexist on the same hardware without port conflicts.
//!
//! For each node:
//! - SSH in and check that both `picloud-production` and `picloud-staging`
//!   systemd services are active.
//! - HTTP GET each service's health endpoint to confirm they respond.
//!
//! If the staging service does not exist on any node, the scenario skips.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::config::NodeConfig;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PortNonConflictScenario;

/// Run an SSH command on a node and return stdout.
async fn ssh_command(node: &NodeConfig, command: &str) -> Result<String, String> {
    let target = format!("{}@{}", node.ssh_user, node.ip);
    let output = tokio::process::Command::new("ssh")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg(&target)
        .arg(command)
        .output()
        .await
        .map_err(|e| format!("ssh to {} failed: {}", node.hostname, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!(
            "ssh command on {} exited {}: stdout={}, stderr={}",
            node.hostname,
            output.status,
            stdout.trim(),
            stderr.trim()
        ));
    }

    Ok(stdout)
}

/// Check if an HTTP health endpoint responds successfully.
async fn check_health(client: &reqwest::Client, ip: &str, port: u16) -> Result<(), String> {
    let url = format!("https://{}:{}/api/health", ip, port);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("health check at {} failed: {}", url, e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "health check at {} returned {}",
            url,
            resp.status()
        ));
    }
    Ok(())
}

#[async_trait]
impl Scenario for PortNonConflictScenario {
    fn name(&self) -> &str {
        "port-non-conflict"
    }

    fn adr(&self) -> &str {
        "ADR-056"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no nodes configured".to_string(),
            };
        }

        let prod_port = ctx.config.cluster.http_port;
        let staging_port: u16 = 8443;

        // Check each node for both services.
        let mut staging_found = false;

        for node in &ctx.config.nodes {
            if node.ssh_user.is_empty() {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "node {} has no ssh_user configured",
                        node.hostname
                    ),
                };
            }

            // Check systemd service status for both production and staging.
            let status_output = match ssh_command(
                node,
                "systemctl is-active picloud-production picloud-staging 2>/dev/null || true",
            )
            .await
            {
                Ok(out) => out,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("SSH to {} failed: {}", node.hostname, e),
                    };
                }
            };

            let lines: Vec<&str> = status_output.trim().lines().collect();

            // systemctl is-active prints one line per service.
            // lines[0] = picloud-production, lines[1] = picloud-staging
            let prod_active = lines.first().map(|l| l.trim()) == Some("active");
            let staging_active = lines.get(1).map(|l| l.trim()) == Some("active");

            if !prod_active {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "picloud-production is not active on {} (status: {:?})",
                        node.hostname,
                        lines.first()
                    ),
                };
            }

            if !staging_active {
                // Staging may not be deployed — check if the unit even exists.
                let unit_check = ssh_command(
                    node,
                    "systemctl list-unit-files picloud-staging.service 2>/dev/null | grep -c picloud-staging || echo 0",
                )
                .await
                .unwrap_or_else(|_| "0".to_string());

                if unit_check.trim() == "0" {
                    return ScenarioResult::Skip {
                        reason: format!(
                            "picloud-staging service does not exist on {}",
                            node.hostname
                        ),
                    };
                }

                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "picloud-staging exists but is not active on {}",
                        node.hostname
                    ),
                };
            }

            staging_found = true;

            info!(
                node = node.hostname.as_str(),
                "both production and staging services active"
            );

            // Verify HTTP health on both ports.
            if let Err(e) = check_health(&ctx.http_client, &node.ip, prod_port).await {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "production health check failed on {}: {}",
                        node.hostname, e
                    ),
                };
            }

            if let Err(e) = check_health(&ctx.http_client, &node.ip, staging_port).await {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "staging health check failed on {}: {}",
                        node.hostname, e
                    ),
                };
            }

            info!(
                node = node.hostname.as_str(),
                prod_port = prod_port,
                staging_port = staging_port,
                "both services responding on their designated ports"
            );
        }

        if !staging_found {
            return ScenarioResult::Skip {
                reason: "staging service not found on any node".to_string(),
            };
        }

        info!(
            nodes = ctx.config.nodes.len(),
            "all nodes have both production and staging services active"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
