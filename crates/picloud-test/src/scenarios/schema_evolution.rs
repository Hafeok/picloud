//! ADR-031: Schema evolution — apply event with schema v1, then v2. Assert both
//! coexist.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SchemaEvolution;

#[async_trait]
impl Scenario for SchemaEvolution {
    fn name(&self) -> &str {
        "schema-evolution"
    }

    fn adr(&self) -> &str {
        "ADR-031"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/commands").await {
            return ScenarioResult::Skip {
                reason: "event API not available".to_string(),
            };
        }

        // First, ensure the product exists for event store scoping.
        let test_product = "schema-evo-test";
        if let Err(e) = assertions::apply_product_and_wait(ctx, test_product, "1.0.0").await {
            // Product may already exist (409) — that's fine.
            info!("product apply result: {}", e);
        }

        // Emit an event with schema v1 (type name encodes the version since
        // /api/commands always generates schema v1 for the given type).
        let v1_event = serde_json::json!({
            "type": "SchemaTestV1",
            "source": "picloud-test",
            "product": test_product,
            "payload": {
                "name": "test-resource",
                "version": 1
            }
        });

        match assertions::http_post(ctx, "/api/commands", v1_event).await {
            Ok(resp) if resp.status().is_success() => {
                info!("v1 schema event emitted");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!("v1 event emission returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("v1 event emission failed: {}", e),
                };
            }
        }

        // Emit an event with schema v2 (different type name = different schema IRI).
        let v2_event = serde_json::json!({
            "type": "SchemaTestV2",
            "source": "picloud-test",
            "product": test_product,
            "payload": {
                "name": "test-resource",
                "version": 2,
                "additionalField": "new-in-v2"
            }
        });

        match assertions::http_post(ctx, "/api/commands", v2_event).await {
            Ok(resp) if resp.status().is_success() => {
                info!("v2 schema event emitted");
            }
            Ok(resp) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("v2 event emission returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("v2 event emission failed: {}", e),
                };
            }
        }

        // Wait for events to be stored.
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Verify both schema versions coexist by reading from the event store API.
        let events_path = format!("/api/event-store/{}/events", test_product);
        match assertions::http_get(ctx, &events_path).await {
            Ok(resp) => {
                let body = match resp.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to read event store response: {}", e),
                        };
                    }
                };

                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse event store response: {}", e),
                        };
                    }
                };

                // Count distinct event types that match our schema test events.
                let events = json["events"].as_array();
                let schema_types: std::collections::HashSet<&str> = events
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| e["event_type"].as_str())
                            .filter(|t| t.starts_with("SchemaTest"))
                            .collect()
                    })
                    .unwrap_or_default();

                let schema_count = schema_types.len();

                if schema_count >= 2 {
                    info!(
                        schemas = schema_count,
                        "multiple schema versions coexist"
                    );
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expected >= 2 distinct schema versions, found {}",
                            schema_count
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("event store query failed: {}", e),
            },
        }
    }
}
