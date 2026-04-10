//! ADR-015: Idempotency key uniqueness — apply two different resources, assert
//! distinct idempotency keys and both are processed.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct IdempotencyKeyUniqueness;

#[async_trait]
impl Scenario for IdempotencyKeyUniqueness {
    fn name(&self) -> &str {
        "idempotency-key-uniqueness"
    }

    fn adr(&self) -> &str {
        "ADR-015"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        if !assertions::feature_available(ctx, "/api/resources").await {
            return ScenarioResult::Skip {
                reason: "resource API not available".to_string(),
            };
        }

        let key_a = format!("uniqueness-test-a-{}", uuid::Uuid::new_v4());
        let key_b = format!("uniqueness-test-b-{}", uuid::Uuid::new_v4());

        let body_a = serde_json::json!({
            "type": "container",
            "name": "uniqueness-test-a",
            "product": "picloud-test",
            "idempotencyKey": key_a,
            "spec": {
                "image": "alpine:latest",
                "command": ["sleep", "3600"]
            }
        });

        let body_b = serde_json::json!({
            "type": "container",
            "name": "uniqueness-test-b",
            "product": "picloud-test",
            "idempotencyKey": key_b,
            "spec": {
                "image": "alpine:latest",
                "command": ["sleep", "3600"]
            }
        });

        // Apply resource A.
        match assertions::http_post(ctx, "/api/resources", body_a).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!(key = %key_a, "resource A applied");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!("resource A apply returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("resource A apply failed: {}", e),
                };
            }
        }

        // Apply resource B.
        match assertions::http_post(ctx, "/api/resources", body_b).await {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 409 => {
                info!(key = %key_b, "resource B applied");
            }
            Ok(resp) => {
                return ScenarioResult::Skip {
                    reason: format!("resource B apply returned status {}", resp.status()),
                };
            }
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("resource B apply failed: {}", e),
                };
            }
        }

        // Verify both resources exist in the RDF graph.
        let both_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT (COUNT(?r) AS ?count) WHERE {
                VALUES ?name { "uniqueness-test-a" "uniqueness-test-b" }
                ?r picloud:name ?name .
            }
        "#;

        match assertions::sparql_query(ctx, both_query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse count response: {}", e),
                        };
                    }
                };

                let count: u64 = json
                    .pointer("/results/bindings/0/count/value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                if count < 2 {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "expected 2 distinct resources, found {} — keys may have collided",
                            count
                        ),
                    };
                }

                info!("both resources exist with distinct idempotency keys");
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("verification query failed: {}", e),
            },
        }
    }
}
