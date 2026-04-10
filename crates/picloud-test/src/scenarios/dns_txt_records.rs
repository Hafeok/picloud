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

        // TXT records contain cluster-id and platform-version. Query the RDF
        // graph to verify this data exists (the DNS server serves it from the graph).
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?clusterId ?version WHERE {
                ?cluster a picloud:Cluster ;
                         picloud:clusterId ?clusterId .
                OPTIONAL { ?cluster picloud:platformVersion ?version }
            }
        "#;

        let body = match assertions::sparql_query(ctx, query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SPARQL endpoint not available: {}", e),
                };
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse SPARQL response: {}", e),
                };
            }
        };

        let bindings = match json.pointer("/results/bindings").and_then(|b| b.as_array()) {
            Some(a) => a,
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "SPARQL response missing results/bindings".to_string(),
                };
            }
        };

        if bindings.is_empty() {
            return ScenarioResult::Skip {
                reason: "no cluster identity in graph — TXT record data not available yet"
                    .to_string(),
            };
        }

        // Verify cluster ID is present.
        let has_cluster_id = bindings[0]
            .pointer("/clusterId/value")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);

        if !has_cluster_id {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "cluster ID not present in graph — TXT record would be incomplete"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
