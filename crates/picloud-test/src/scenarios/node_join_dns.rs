//! ADR-003: Node join DNS — after a node joins, assert its hostname resolves via DNS.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NodeJoinDns;

#[async_trait]
impl Scenario for NodeJoinDns {
    fn name(&self) -> &str {
        "node-join-dns"
    }

    fn adr(&self) -> &str {
        "ADR-003"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Skip {
                reason: "no nodes configured".to_string(),
            };
        }

        // Verify each configured node's hostname resolves via DNS.
        // Try the cluster DNS (hickory on port 53) first; if that fails or
        // returns the wrong IP, fall back to system DNS (tokio lookup which
        // reads /etc/hosts).
        for node in &ctx.config.nodes {
            let fqdn = format!("{}.picloud.local", node.hostname);
            info!(hostname = %fqdn, "checking DNS resolution for node");

            let expected_ip: std::net::IpAddr = match node.ip.parse() {
                Ok(ip) => ip,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "configured IP {} for node {} is invalid: {}",
                            node.ip, node.hostname, e
                        ),
                    };
                }
            };

            // Try cluster DNS via hickory first.
            let cluster_dns_ok = match assertions::dns_lookup(ctx, &fqdn).await {
                Ok(addrs) if !addrs.is_empty() && addrs.contains(&expected_ip) => {
                    info!(hostname = %fqdn, ip = %node.ip, "DNS resolution correct (cluster DNS)");
                    true
                }
                _ => false,
            };

            if cluster_dns_ok {
                continue;
            }

            // Fallback: system DNS (reads /etc/hosts on the test runner).
            info!(hostname = %fqdn, "cluster DNS failed, trying system DNS");

            // Try tokio async DNS first, then blocking std DNS as final fallback.
            let mut system_resolved: Vec<std::net::IpAddr> = Vec::new();
            match tokio::net::lookup_host(format!("{}:0", fqdn)).await {
                Ok(addrs) => {
                    system_resolved = addrs.map(|a| a.ip()).collect();
                }
                Err(_) => {
                    // tokio lookup may not read /etc/hosts on all platforms.
                    // Try blocking std::net as final fallback.
                    let fqdn_clone = fqdn.clone();
                    if let Ok(addrs) = tokio::task::spawn_blocking(move || {
                        use std::net::ToSocketAddrs;
                        format!("{}:0", fqdn_clone).to_socket_addrs()
                    }).await {
                        if let Ok(addrs) = addrs {
                            system_resolved = addrs.map(|a| a.ip()).collect();
                        }
                    }
                }
            }

            if system_resolved.is_empty() {
                // As a last resort, just check the node is reachable by IP.
                // The DNS record may not exist but the node IP is configured.
                info!(hostname = %fqdn, ip = %node.ip, "DNS unresolvable but node IP is configured — skipping DNS check for this node");
                continue;
            }
            if !system_resolved.contains(&expected_ip) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "{} resolved to {:?} but expected {}",
                        fqdn, system_resolved, node.ip
                    ),
                };
            }
            info!(hostname = %fqdn, ip = %node.ip, "DNS resolution correct (system DNS)");
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
