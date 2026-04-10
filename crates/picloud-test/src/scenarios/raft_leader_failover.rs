//! ADR-002: Raft leader failover — SSH kill leader, wait for new leader, assert re-election < 5s.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RaftLeaderFailover;

#[async_trait]
impl Scenario for RaftLeaderFailover {
    fn name(&self) -> &str {
        "raft-leader-failover"
    }

    fn adr(&self) -> &str {
        "ADR-002"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if ctx.config.nodes.len() < 2 {
            return ScenarioResult::Skip {
                reason: "need at least 2 nodes for leader failover test".to_string(),
            };
        }

        // Identify the current leader via SPARQL.
        let leader_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?node WHERE {
                ?node picloud:hasRole picloud:Leader .
            }
        "#;

        let leader_body = match assertions::sparql_query(ctx, leader_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to query leader: {}", e),
                };
            }
        };

        let leader_json: serde_json::Value = match serde_json::from_str(&leader_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse leader SPARQL response: {}", e),
                };
            }
        };

        let leader_iri = match leader_json
            .pointer("/results/bindings/0/node/value")
            .and_then(|v| v.as_str())
        {
            Some(iri) => iri.to_string(),
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "no leader found in SPARQL response".to_string(),
                };
            }
        };

        info!(leader = %leader_iri, "identified current leader");

        // Find the node config matching the leader IRI.
        let leader_node = ctx.config.nodes.iter().find(|n| leader_iri.contains(&n.hostname));
        let leader_node = match leader_node {
            Some(n) => n,
            None => {
                return ScenarioResult::Skip {
                    reason: format!(
                        "leader IRI {} does not match any configured node hostname",
                        leader_iri
                    ),
                };
            }
        };

        // Kill picloud-server on the leader node via SSH.
        info!(node = %leader_node.hostname, "killing picloud-server on leader");
        match assertions::ssh_command(leader_node, "sudo pkill -9 picloud-server").await {
            Ok(_) => {}
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SSH kill command failed: {}", e),
                };
            }
        }

        // Wait for a new leader to be elected within 5 seconds.
        let election_start = Instant::now();
        let election_timeout = Duration::from_secs(5);

        let new_leader_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                ?node picloud:hasRole picloud:Leader .
                FILTER(?node != <{}>)
            }}
            "#,
            leader_iri
        );

        match assertions::wait_for_sparql(ctx, &new_leader_query, election_timeout).await {
            Ok(()) => {
                let election_duration = election_start.elapsed();
                info!(
                    elapsed_ms = election_duration.as_millis() as u64,
                    "new leader elected"
                );

                if election_duration > Duration::from_secs(5) {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "leader re-election took {:?}, exceeds 5s threshold",
                            election_duration
                        ),
                    };
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("new leader not elected within 5s: {}", e),
            },
        }
    }
}
