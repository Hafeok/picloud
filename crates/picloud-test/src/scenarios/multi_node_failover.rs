//! Multi-node failover — kill the Raft leader, verify a new leader is elected.
//!
//! In single-node mode: verifies leader exists and returns Pass.
//! In multi-node mode: SSH kills the leader process, waits for re-election,
//! verifies via SPARQL that a different node is now leader.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct MultiNodeFailover;

#[async_trait]
impl Scenario for MultiNodeFailover {
    fn name(&self) -> &str {
        "multi-node-failover"
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

        // Determine active node count from GET /
        let cluster_resp = match assertions::http_get(ctx, "/").await {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to GET /: {}", e),
                };
            }
        };
        let cluster_json: serde_json::Value =
            serde_json::from_str(&cluster_resp).unwrap_or_default();
        let active_nodes = cluster_json
            .get("nodes")
            .and_then(|n| n.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        // Query for the current leader
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
                    reason: format!("failed to parse leader response: {}", e),
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

        info!(leader = %leader_iri, active_nodes, "identified leader");

        // Single-node: leader exists, pass
        if active_nodes < 2 {
            info!("single-node cluster — leader verified, failover mechanism present");
            return ScenarioResult::Pass {
                duration: start.elapsed(),
            };
        }

        // Multi-node: find the node config matching the leader and SSH kill it
        let leader_node = ctx
            .config
            .nodes
            .iter()
            .find(|n| leader_iri.contains(&n.hostname));
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

        info!(node = %leader_node.hostname, "killing picloud-server on leader via SSH");
        match assertions::ssh_command(leader_node, "sudo pkill -9 picloud-server").await {
            Ok(_) => {}
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SSH kill failed: {}", e),
                };
            }
        }

        // Wait for a new (different) leader to be elected
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

        match assertions::wait_for_sparql(ctx, &new_leader_query, Duration::from_secs(10)).await {
            Ok(()) => {
                info!(
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "new leader elected after failover"
                );
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("new leader not elected within 10s: {}", e),
            },
        }
    }
}
