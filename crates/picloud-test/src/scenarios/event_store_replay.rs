//! ADR-032: Event store replay — POST events, request replay from index 0.
//! Assert all present.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct EventStoreReplay;

#[async_trait]
impl Scenario for EventStoreReplay {
    fn name(&self) -> &str {
        "event-store-replay"
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

        let batch_id = format!("replay-batch-{}", uuid::Uuid::new_v4());
        let event_count = 5;

        // Append several events.
        for i in 0..event_count {
            let event_body = serde_json::json!({
                "type": "ReplayTestEvent",
                "aggregateType": "ReplayTest",
                "aggregateId": format!("replay-{}-{}", batch_id, i),
                "schema": "https://picloud.local/schemas/events/ReplayTestEvent/v1",
                "payload": {
                    "batchId": batch_id,
                    "index": i
                }
            });

            match assertions::http_post(
                ctx,
                "/api/event-store/picloud-test/append",
                event_body,
            )
            .await
            {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    return ScenarioResult::Skip {
                        reason: format!(
                            "event store append returned status {} on event {}",
                            resp.status(),
                            i
                        ),
                    };
                }
                Err(e) => {
                    return ScenarioResult::Skip {
                        reason: format!("event store append failed on event {}: {}", i, e),
                    };
                }
            }
        }

        info!(count = event_count, batch = %batch_id, "events appended");

        // Wait briefly.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Request replay from the beginning.
        let replay_body = serde_json::json!({
            "from": "1970-01-01T00:00:00Z"
        });

        match assertions::http_post(
            ctx,
            "/api/event-store/picloud-test/replay",
            replay_body,
        )
        .await
        {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "event store replay endpoint not implemented".to_string(),
                    };
                }

                if !resp.status().is_success() {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("replay endpoint returned status {}", status),
                    };
                }

                let body = resp.text().await.unwrap_or_default();

                // Count how many of our batch events appear in the replay.
                let mut found = 0;
                for i in 0..event_count {
                    let expected_id = format!("replay-{}-{}", batch_id, i);
                    if body.contains(&expected_id) {
                        found += 1;
                    }
                }

                if found == event_count {
                    info!("all {} events found in replay", event_count);
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "only {}/{} events found in replay response",
                            found, event_count
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("replay request failed: {}", e),
            },
        }
    }
}
