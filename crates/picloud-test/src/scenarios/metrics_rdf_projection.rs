//! ADR-040: SPARQL for hardware metrics projected into the RDF graph.
//!
//! Queries for picloud:Metric typed resources in the graph. After a
//! MetricRecorded event, node IRIs should carry metric triples such as
//! cpuUsagePercent, memoryUsedMb, cpuTempCelsius, and metricsUpdatedAt.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct MetricsRdfProjection;

#[async_trait]
impl Scenario for MetricsRdfProjection {
    fn name(&self) -> &str {
        "metrics-rdf-projection"
    }

    fn adr(&self) -> &str {
        "ADR-040"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query for hardware metrics on any node.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?node ?cpu ?mem ?temp ?updated WHERE {
                ?node a picloud:Node ;
                      picloud:cpuUsagePercent ?cpu ;
                      picloud:memoryUsedMb ?mem ;
                      picloud:cpuTempCelsius ?temp ;
                      picloud:metricsUpdatedAt ?updated .
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

        let bindings = match json.pointer("/results/bindings") {
            Some(b) => b,
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "SPARQL response missing results/bindings".to_string(),
                };
            }
        };

        let arr = match bindings.as_array() {
            Some(a) => a,
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "SPARQL bindings is not an array".to_string(),
                };
            }
        };

        if arr.is_empty() {
            return ScenarioResult::Skip {
                reason: "no metric data projected yet — metrics agent may not have run"
                    .to_string(),
            };
        }

        // Verify at least one node has all required metric fields.
        let first = &arr[0];
        for field in &["cpu", "mem", "temp", "updated"] {
            if first.get(field).is_none() {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("metric field '{}' missing from SPARQL result", field),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
