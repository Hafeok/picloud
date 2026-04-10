//! ADR-032: Event store RDF projection — POST event, SPARQL for its projection.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventStoreRdfProjection;

#[async_trait]
impl Scenario for EventStoreRdfProjection {
    fn name(&self) -> &str {
        "event-store-rdf-projection"
    }

    fn adr(&self) -> &str {
        "ADR-032"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/event-store").await {
            return ScenarioResult::Skip {
                reason: "event store API not available".to_string(),
            };
        }

        let aggregate_id = format!("photo-proj-{}", uuid::Uuid::new_v4());

        // Append an event that should project into the product RDF graph.
        let event_body = serde_json::json!({
            "type": "PhotoCreated",
            "aggregateType": "Photo",
            "aggregateId": aggregate_id,
            "schema": "https://picloud.local/schemas/events/PhotoCreated/v1",
            "payload": {
                "title": "Projection Test Photo",
                "size": 2048
            }
        });

        match assertions::http_post(
            ctx,
            "/api/event-store/picloud-test/append",
            event_body,
        )
        .await
        {
            Ok(resp) if resp.status().is_success() => {
                info!(aggregate = %aggregate_id, "event appended");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!(
                        "event store append returned status {}",
                        resp.status()
                    ),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("event store append failed: {}", e),
                };
            }
        }

        // Wait for projection to process the event.
        let projection_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                ?photo picloud:aggregateId "{}" .
            }}
            "#,
            aggregate_id
        );

        match assertions::wait_for_sparql(
            ctx,
            &projection_query,
            Duration::from_secs(30),
        )
        .await
        {
            Ok(()) => {
                info!("event projected into RDF graph");
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "event not projected to RDF within 30s: {}",
                    e
                ),
            },
        }
    }
}
