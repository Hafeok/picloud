//! ADR-020: Cluster registry discovery — GET / and assert JSON-LD response
//! with cluster type and nodes array.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ClusterRegistryDiscovery;

#[async_trait]
impl Scenario for ClusterRegistryDiscovery {
    fn name(&self) -> &str {
        "cluster-registry-discovery"
    }

    fn adr(&self) -> &str {
        "ADR-020"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // GET the cluster root.
        match assertions::http_get(ctx, "/").await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let content_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let body = resp.text().await.unwrap_or_default();

                if status != 200 {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("cluster root returned {} (expected 200)", status),
                    };
                }

                // Parse as JSON and check for expected structure.
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "cluster root is not valid JSON: {}",
                                e
                            ),
                        };
                    }
                };

                let mut issues: Vec<String> = Vec::new();

                // Check for JSON-LD context or type field.
                let has_context = json.get("@context").is_some();
                let has_type = json.get("@type").is_some()
                    || json.get("type").is_some();
                let has_jsonld_content_type = content_type.contains("ld+json");

                if !has_context && !has_jsonld_content_type {
                    issues.push(
                        "response lacks @context and is not served as application/ld+json"
                            .to_string(),
                    );
                }

                if !has_type {
                    issues.push(
                        "response lacks @type or type field".to_string(),
                    );
                }

                // Check for nodes presence.
                let has_nodes = json.get("nodes").is_some()
                    || json.get("members").is_some();
                if !has_nodes {
                    issues.push(
                        "response lacks nodes/members array".to_string(),
                    );
                }

                if issues.is_empty() {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "{} registry issue(s): {}",
                            issues.len(),
                            issues.join("; ")
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to GET cluster root: {}", e),
            },
        }
    }
}
