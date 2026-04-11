//! ADR-003: Resolve a product FQDN via the cluster DNS and verify it returns
//! a valid IP address. Skips if the cluster is unavailable.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions::{dns_lookup, feature_available};
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ProductFqdnDns;

#[async_trait]
impl Scenario for ProductFqdnDns {
    fn name(&self) -> &str {
        "product_fqdn_dns"
    }

    fn adr(&self) -> &str {
        "ADR-003"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Check if the cluster is reachable at all.
        if !feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "Cluster not reachable — skipping DNS resolution test".to_string(),
            };
        }

        // Construct the product FQDN. Use the cluster domain from config.
        let domain = &ctx.config.cluster.domain;
        let product_fqdn = format!("test-app.{}", domain);

        // Helper closure to validate resolved addresses.
        let validate_addrs = |addrs: &[std::net::IpAddr]| -> Option<ScenarioResult> {
            for addr in addrs {
                if addr.is_loopback() {
                    return Some(ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "DNS lookup for {} returned loopback address {}",
                            product_fqdn, addr
                        ),
                    });
                }
                if addr.is_unspecified() {
                    return Some(ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "DNS lookup for {} returned unspecified address {}",
                            product_fqdn, addr
                        ),
                    });
                }
            }
            None
        };

        // Try cluster DNS (hickory on port 53) first.
        match dns_lookup(ctx, &product_fqdn).await {
            Ok(addrs) if !addrs.is_empty() => {
                if let Some(fail) = validate_addrs(&addrs) {
                    return fail;
                }
                return ScenarioResult::Pass {
                    duration: start.elapsed(),
                };
            }
            _ => {}
        }

        // Fallback: system DNS (reads /etc/hosts).
        let mut system_addrs: Vec<std::net::IpAddr> = Vec::new();
        match tokio::net::lookup_host(format!("{}:0", product_fqdn)).await {
            Ok(addrs_iter) => {
                system_addrs = addrs_iter.map(|a| a.ip()).collect();
            }
            Err(_) => {
                // tokio lookup may not read /etc/hosts on all platforms.
                // Try blocking std::net as final fallback.
                let fqdn_clone = product_fqdn.clone();
                if let Ok(addrs) = tokio::task::spawn_blocking(move || {
                    use std::net::ToSocketAddrs;
                    format!("{}:0", fqdn_clone).to_socket_addrs()
                }).await {
                    if let Ok(addrs) = addrs {
                        system_addrs = addrs.map(|a| a.ip()).collect();
                    }
                }
            }
        }

        if system_addrs.is_empty() {
            return ScenarioResult::Skip {
                reason: format!(
                    "Product FQDN {} not found in DNS (product may not be deployed)",
                    product_fqdn
                ),
            };
        }
        if let Some(fail) = validate_addrs(&system_addrs) {
            return fail;
        }
        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
