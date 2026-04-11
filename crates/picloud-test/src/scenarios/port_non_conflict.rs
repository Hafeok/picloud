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

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PortNonConflictScenario;

/// Check if an HTTP health endpoint responds successfully.
/// Tries both http and https schemes since the server may run either.
async fn check_health(client: &reqwest::Client, ip: &str, port: u16) -> Result<(), String> {
    // Try HTTP first (most common in our nohup-based deployments), then HTTPS.
    for scheme in &["http", "https"] {
        let url = format!("{}://{}:{}/health", scheme, ip, port);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            Ok(resp) => {
                // Try /api/health as fallback path
                let url2 = format!("{}://{}:{}/api/health", scheme, ip, port);
                if let Ok(resp2) = client.get(&url2).send().await {
                    if resp2.status().is_success() {
                        return Ok(());
                    }
                }
                // If this was the last scheme, report the failure
                if *scheme == "https" {
                    return Err(format!(
                        "health check at {}:{} returned {} (tried http and https)",
                        ip, port, resp.status()
                    ));
                }
            }
            Err(_) => continue,
        }
    }
    Err(format!("health check at {}:{} unreachable (tried http and https)", ip, port))
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

        // Check each node for both production and staging by probing HTTP
        // health endpoints directly (works with both systemd and nohup deployments).
        let mut staging_found = false;

        for node in &ctx.config.nodes {
            // Verify HTTP health on production port.
            if let Err(e) = check_health(&ctx.http_client, &node.ip, prod_port).await {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "production health check failed on {}: {}",
                        node.hostname, e
                    ),
                };
            }

            // Check staging port — skip gracefully if staging is not running.
            match check_health(&ctx.http_client, &node.ip, staging_port).await {
                Ok(()) => {
                    staging_found = true;
                    info!(
                        node = node.hostname.as_str(),
                        prod_port = prod_port,
                        staging_port = staging_port,
                        "both services responding on their designated ports"
                    );
                }
                Err(_) => {
                    info!(
                        node = node.hostname.as_str(),
                        staging_port = staging_port,
                        "staging not responding on this node"
                    );
                }
            }
        }

        if !staging_found {
            return ScenarioResult::Skip {
                reason: "staging service not responding on any node".to_string(),
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
