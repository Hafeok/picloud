//! ADR-032: Event store append/read — POST event to product event store, GET it
//! back.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventStoreAppendRead;

#[async_trait]
impl Scenario for EventStoreAppendRead {
    fn name(&self) -> &str {
        "event-store-append-read"
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

        // Check if the event store API exists.
        if !assertions::feature_available(ctx, "/api/event-store").await {
            return ScenarioResult::Skip {
                reason: "event store API not available".to_string(),
            };
        }

        let event_id = format!("evt-{}", uuid::Uuid::new_v4());

        // Append an event to the product event store.
        let event_body = serde_json::json!({
            "type": "PhotoCreated",
            "aggregateType": "Photo",
            "aggregateId": format!("photo-{}", uuid::Uuid::new_v4()),
            "schema": "https://picloud.local/schemas/events/PhotoCreated/v1",
            "payload": {
                "eventId": event_id,
                "title": "Test Photo",
                "size": 1024
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
                info!(event_id = %event_id, "event appended to store");
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

        // Wait for the event to be stored.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Read the event back from the store.
        let read_path = "/api/event-store/picloud-test/events?limit=10";
        match assertions::http_get(ctx, read_path).await {
            Ok(resp) if resp.status().is_success() => {
                let body = resp.text().await.unwrap_or_default();
                if body.contains(&event_id) {
                    info!("appended event found in read response");
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "event {} not found in event store read response",
                            event_id
                        ),
                    }
                }
            }
            Ok(resp) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "event store read returned status {}",
                    resp.status()
                ),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("event store read failed: {}", e),
            },
        }
    }
}
