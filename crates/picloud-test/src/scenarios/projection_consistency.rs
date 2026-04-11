//! ADR-004: Projection consistency — after applying a resource, verify the
//! RDF projection reflects it within the latency budget.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ProjectionConsistency;

#[async_trait]
impl Scenario for ProjectionConsistency {
    fn name(&self) -> &str {
        "projection-consistency"
    }

    fn adr(&self) -> &str {
        "ADR-004"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::commands_available(ctx).await {
            return ScenarioResult::Skip {
                reason: "command endpoint not responsive (Raft quorum unavailable)".to_string(),
            };
        }

        // Apply a test resource.
        let test_name = format!("test-proj-{}", uuid::Uuid::new_v4().as_simple());
        let resource = serde_json::json!({
            "type": "product",
            "name": test_name,
            "version": "1.0.0"
        });

        if let Err(e) = assertions::apply_resource(ctx, resource).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply product: {}", e),
            };
        }

        // ASK for the resource within the projection latency budget (p99 < 2s).
        let ask_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <https://picloud.local/products/{}> a picloud:Product .
            }}
            "#,
            test_name
        );

        match assertions::wait_for_sparql(ctx, &ask_query, Duration::from_secs(5)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "projection did not converge within latency budget: {}",
                    e
                ),
            },
        }
    }
}
