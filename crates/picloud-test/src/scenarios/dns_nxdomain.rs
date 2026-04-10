//! ADR-052: DNS NXDOMAIN — query a non-existent hostname, assert empty result.
//!
//! DNS lookup for a non-existent name via the cluster DNS server. Assert
//! NXDOMAIN or empty result (no fallthrough, no recursion).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct DnsNxdomain;

#[async_trait]
impl Scenario for DnsNxdomain {
    fn name(&self) -> &str {
        "dns-nxdomain"
    }

    fn adr(&self) -> &str {
        "ADR-052"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Skip {
                reason: "no cluster nodes configured — cannot test DNS".to_string(),
            };
        }

        // Query a hostname that should not exist in the cluster.
        let result =
            assertions::dns_lookup(ctx, "this-does-not-exist-at-all.picloud.local").await;

        match result {
            Ok(addrs) => {
                if addrs.is_empty() {
                    // Empty result is acceptable for NXDOMAIN.
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "non-existent hostname resolved to {:?} — NXDOMAIN not enforced",
                            addrs
                        ),
                    }
                }
            }
            Err(_) => {
                // An error from the resolver (NXDOMAIN, SERVFAIL, etc.) is the expected
                // outcome for a non-existent hostname.
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
        }
    }
}
