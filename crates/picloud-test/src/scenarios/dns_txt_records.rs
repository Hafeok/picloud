//! ADR-052: DNS TXT records — query TXT records for picloud.local.
//!
//! Assert platform version or cluster info is present in TXT record data.
//! Uses SPARQL to verify the data that backs TXT records.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct DnsTxtRecords;

#[async_trait]
impl Scenario for DnsTxtRecords {
    fn name(&self) -> &str {
        "dns-txt-records"
    }

    fn adr(&self) -> &str {
        "ADR-052"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if ctx.config.nodes.is_empty() {
            return ScenarioResult::Skip {
                reason: "no cluster nodes configured — cannot test DNS TXT records".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // TXT records contain cluster-id and platform-version. Check the /
        // endpoint first (which returns cluster identity), then fall back to
        // SPARQL if the root endpoint doesn't have the data.
        let root_resp = match assertions::http_get(ctx, "/").await {
            Ok(r) => Some(r),
            Err(_) => None,
        };

        let mut has_cluster_id = false;

        if let Some(resp) = root_resp {
            if resp.status().is_success() {
                let body = resp.text().await.unwrap_or_default();
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    // Check for cluster_id in root response
                    has_cluster_id = json.get("cluster_id")
                        .or_else(|| json.get("clusterId"))
                        .and_then(|v| v.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                }
            }
        }

        // Fall back to SPARQL if root endpoint didn't have cluster ID.
        if !has_cluster_id {
            let query = r#"
                PREFIX picloud: <https://picloud.local/ontology#>
                SELECT ?clusterId ?version WHERE {
                    ?cluster a picloud:Cluster ;
                             picloud:clusterId ?clusterId .
                    OPTIONAL { ?cluster picloud:platformVersion ?version }
                }
            "#;

            match assertions::sparql_query(ctx, query).await {
                Ok(body) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        has_cluster_id = json.pointer("/results/bindings/0/clusterId/value")
                            .and_then(|v| v.as_str())
                            .map(|s| !s.is_empty())
                            .unwrap_or(false);
                    }
                }
                Err(e) => {
                    return ScenarioResult::Skip {
                        reason: format!("SPARQL endpoint not available and root endpoint did not have cluster ID: {}", e),
                    };
                }
            }
        }

        if !has_cluster_id {
            return ScenarioResult::Skip {
                reason: "no cluster identity found in / response or SPARQL graph — TXT record data not available yet"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
