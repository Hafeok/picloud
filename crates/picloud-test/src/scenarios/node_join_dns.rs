//! ADR-003: Node join DNS — after a node joins, assert its hostname resolves via DNS.

use std::time::{Duration, Instant};

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

        // Verify each configured node's hostname resolves via the cluster DNS.
        for node in &ctx.config.nodes {
            let fqdn = format!("{}.picloud.local", node.hostname);
            info!(hostname = %fqdn, "checking DNS resolution for node");

            match assertions::dns_lookup(ctx, &fqdn).await {
                Ok(addrs) => {
                    if addrs.is_empty() {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("{} resolved but returned no addresses", fqdn),
                        };
                    }

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

                    if !addrs.contains(&expected_ip) {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "{} resolved to {:?} but expected {}",
                                fqdn, addrs, node.ip
                            ),
                        };
                    }

                    info!(hostname = %fqdn, ip = %node.ip, "DNS resolution correct");
                }
                Err(e) => {
                    // Check if node has joined by looking at the RDF graph.
                    let ask_query = format!(
                        r#"
                        PREFIX picloud: <https://picloud.local/ontology#>
                        ASK {{
                            <https://picloud.local/nodes/{}> a picloud:Node .
                        }}
                        "#,
                        node.hostname
                    );

                    match assertions::wait_for_sparql(
                        ctx,
                        &ask_query,
                        Duration::from_secs(60),
                    )
                    .await
                    {
                        Ok(()) => {
                            // Node is in the graph but DNS failed — that is a real failure.
                            return ScenarioResult::Fail {
                                duration: start.elapsed(),
                                reason: format!(
                                    "node {} is in RDF graph but DNS resolution failed: {}",
                                    node.hostname, e
                                ),
                            };
                        }
                        Err(_) => {
                            return ScenarioResult::Skip {
                                reason: format!(
                                    "node {} not found in cluster and DNS unresolvable: {}",
                                    node.hostname, e
                                ),
                            };
                        }
                    }
                }
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
