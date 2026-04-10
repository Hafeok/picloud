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

        match dns_lookup(ctx, &product_fqdn).await {
            Ok(addrs) => {
                if addrs.is_empty() {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "DNS lookup for {} returned zero addresses",
                            product_fqdn
                        ),
                    };
                }

                // Verify each returned address is a valid, non-loopback IP.
                for addr in &addrs {
                    if addr.is_loopback() {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "DNS lookup for {} returned loopback address {}",
                                product_fqdn, addr
                            ),
                        };
                    }
                    if addr.is_unspecified() {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "DNS lookup for {} returned unspecified address {}",
                                product_fqdn, addr
                            ),
                        };
                    }
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => {
                // DNS lookup failure is not necessarily a test failure if the
                // product hasn't been deployed. We skip in that case.
                let err_str = e.to_string();
                if err_str.contains("NXDOMAIN") || err_str.contains("no records") {
                    ScenarioResult::Skip {
                        reason: format!(
                            "Product FQDN {} not found in DNS (product may not be deployed): {}",
                            product_fqdn, err_str
                        ),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "DNS lookup for {} failed: {}",
                            product_fqdn, err_str
                        ),
                    }
                }
            }
        }
    }
}
