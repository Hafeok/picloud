//! ADR-004: Event log replay — POST a test resource command, then verify
//! it appears in the RDF projection via SPARQL.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventLogReplay;

#[async_trait]
impl Scenario for EventLogReplay {
    fn name(&self) -> &str {
        "event-log-replay"
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

        // Apply a test product via the resource apply endpoint.
        let test_product = format!("test-replay-{}", uuid::Uuid::new_v4().as_simple());
        let resource = serde_json::json!({
            "type": "product",
            "name": test_product,
            "version": "1.0.0"
        });

        if let Err(e) = assertions::apply_resource(ctx, resource).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply product: {}", e),
            };
        }

        // Wait for the projection to catch up, then query for the resource.
        let ask_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <https://picloud.local/products/{}> a picloud:Product .
            }}
            "#,
            test_product
        );

        match assertions::wait_for_sparql(ctx, &ask_query, Duration::from_secs(10)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "resource not projected after event log replay: {}",
                    e
                ),
            },
        }
    }
}
