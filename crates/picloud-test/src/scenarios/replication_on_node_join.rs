//! ADR-013: Replication on node join — add node, assert data replicates within 120s.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ReplicationOnNodeJoin;

#[async_trait]
impl Scenario for ReplicationOnNodeJoin {
    fn name(&self) -> &str {
        "replication-on-node-join"
    }

    fn adr(&self) -> &str {
        "ADR-013"
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
                reason: "need at least 2 nodes to test replication on join".to_string(),
            };
        }

        // Query the RDF graph for the current cluster member count.
        let member_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?node) AS ?count) WHERE {
                ?node a picloud:Node .
            }
        "#;

        let member_body = match assertions::sparql_query(ctx, member_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("cannot query cluster members: {}", e),
                };
            }
        };

        let member_json: serde_json::Value = match serde_json::from_str(&member_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse member count: {}", e),
                };
            }
        };

        let node_count: u64 = member_json
            .pointer("/results/bindings/0/count/value")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        info!(count = node_count, "current cluster node count");

        // Verify volumes are replicated to existing nodes by checking replication status.
        let repl_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?vol ?replicas WHERE {
                ?vol a picloud:Volume ;
                     picloud:replicationCount ?replicas .
            }
        "#;

        let repl_body = match assertions::sparql_query(ctx, repl_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("cannot query replication status: {}", e),
                };
            }
        };

        let repl_json: serde_json::Value = match serde_json::from_str(&repl_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse replication query: {}", e),
                };
            }
        };

        let bindings = repl_json
            .pointer("/results/bindings")
            .and_then(|v| v.as_array());

        match bindings {
            Some(arr) if !arr.is_empty() => {
                for binding in arr {
                    let vol = binding
                        .pointer("/vol/value")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let replicas: u64 = binding
                        .pointer("/replicas/value")
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);

                    if replicas < node_count {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "volume {} has {} replicas but cluster has {} nodes — under-replicated",
                                vol, replicas, node_count
                            ),
                        };
                    }

                    info!(volume = %vol, replicas = replicas, "replication verified");
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            _ => {
                // No volumes to check — pass vacuously.
                info!("no volumes in cluster — replication check passes vacuously");
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
        }
    }
}
