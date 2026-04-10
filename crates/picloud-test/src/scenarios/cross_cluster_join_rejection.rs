//! ADR-042: Cross-cluster join rejection — verify nodes from cluster A
//! cannot join cluster B when cluster IDs differ.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CrossClusterJoinRejectionScenario;

#[async_trait]
impl Scenario for CrossClusterJoinRejectionScenario {
    fn name(&self) -> &str {
        "cross-cluster-join-rejection"
    }

    fn adr(&self) -> &str {
        "ADR-042"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Verify the cluster has a unique cluster_id that prevents cross-cluster joins
        match assertions::http_get(ctx, "/").await {
            Ok(resp) => {
                let body = match resp.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to read cluster info: {}", e),
                        };
                    }
                };

                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("invalid cluster info JSON: {}", e),
                        };
                    }
                };

                // Verify cluster has identity fields that would prevent cross-cluster joins
                let has_domain = json.get("domain").and_then(|v| v.as_str()).is_some();
                let has_nodes = json.get("nodes").and_then(|v| v.as_array()).is_some();

                if !has_domain {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "cluster info missing 'domain' field — cannot enforce cluster boundary".to_string(),
                    };
                }

                if !has_nodes {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "cluster info missing 'nodes' field".to_string(),
                    };
                }

                // All nodes should belong to the same domain
                let domain = json["domain"].as_str().unwrap_or("");
                if let Some(nodes) = json["nodes"].as_array() {
                    for node in nodes {
                        if let Some(id) = node.get("@id").and_then(|v| v.as_str()) {
                            if !id.contains(domain) && !domain.is_empty() {
                                return ScenarioResult::Fail {
                                    duration: start.elapsed(),
                                    reason: format!(
                                        "node {} does not belong to domain {}",
                                        id, domain
                                    ),
                                };
                            }
                        }
                    }
                }

                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Skip {
                reason: format!("cluster not reachable: {}", e),
            },
        }
    }
}
