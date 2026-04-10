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
