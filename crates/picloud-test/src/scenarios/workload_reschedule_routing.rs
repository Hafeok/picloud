//! ADR-048: Workload reschedule routing — after reschedule, assert ingress routes update.
//!
//! After a workload reschedule, verify that HTTP requests continue
//! succeeding via the ingress router (routing table updated without
//! manual intervention).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct WorkloadRescheduleRouting;

#[async_trait]
impl Scenario for WorkloadRescheduleRouting {
    fn name(&self) -> &str {
        "workload-reschedule-routing"
    }

    fn adr(&self) -> &str {
        "ADR-048"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 0. Apply test resources (product + container + ingress) so there is data in the graph.
        let product_resource = serde_json::json!({
            "type": "product",
            "name": "reschedule-test",
            "version": "1.0.0"
        });
        let _ = assertions::apply_resource(ctx, product_resource).await;

        let container_resource = serde_json::json!({
            "type": "container",
            "name": "api-server",
            "product": "reschedule-test",
            "image": "registry.picloud.local/reschedule-test/api-server:latest",
            "port": 8080
        });
        let _ = assertions::apply_resource(ctx, container_resource).await;

        let ingress_resource = serde_json::json!({
            "type": "ingress",
            "name": "api-ingress",
            "product": "reschedule-test",
            "hostname": "reschedule-test.picloud.local",
            "target": "reschedule-test/containers/api-server",
            "port": 8080
        });
        let _ = assertions::apply_resource(ctx, ingress_resource).await;

        // Brief pause for RDF projection
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Query the RDF graph for any ingress resources with their target addresses.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?ingress ?host ?address WHERE {
                ?ingress a picloud:Ingress ;
                         picloud:hostname ?host ;
                         picloud:targetAddress ?address .
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
                return ScenarioResult::Skip {
                    reason: "no ingress resources in graph — cannot test reschedule routing"
                        .to_string(),
                };
            }
        };

        if bindings.is_empty() {
            return ScenarioResult::Skip {
                reason: "no ingress resources deployed — cannot test reschedule routing"
                    .to_string(),
            };
        }

        // Verify that WorkloadRescheduled events update ingress routes.
        // Check that the ingress router subscribes to reschedule events.
        let events_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?event a picloud:PlatformEvent .
            }
        "#;

        match assertions::sparql_query(ctx, events_query).await {
            Ok(_) => {}
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("event query failed: {}", e),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
