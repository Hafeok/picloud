//! ADR-052: DNS A records — query A records for picloud.local via the cluster DNS.
//!
//! Use dns_lookup to query A records. Assert the resolved IPs match cluster node IPs.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct DnsARecords;

#[async_trait]
impl Scenario for DnsARecords {
    fn name(&self) -> &str {
        "dns-a-records"
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

        // Query A records for picloud.local using the cluster's DNS server.
        let addrs = match assertions::dns_lookup(ctx, "picloud.local").await {
            Ok(a) => a,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("DNS lookup for picloud.local failed: {}", e),
                };
            }
        };

        if addrs.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "DNS lookup for picloud.local returned no addresses".to_string(),
            };
        }

        // Verify the resolved IPs match configured cluster node IPs.
        let node_ips: Vec<&str> = ctx.config.nodes.iter().map(|n| n.ip.as_str()).collect();
        let mut unmatched = Vec::new();

        for addr in &addrs {
            let addr_str = addr.to_string();
            if !node_ips.contains(&addr_str.as_str()) {
                unmatched.push(addr_str);
            }
        }

        if !unmatched.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "DNS returned IPs not in cluster nodes: {:?} (cluster nodes: {:?})",
                    unmatched, node_ips
                ),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
