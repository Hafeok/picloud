//! ADR-037: Try to create circular group membership. Assert rejection.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CircularGroupRejection;

#[async_trait]
impl Scenario for CircularGroupRejection {
    fn name(&self) -> &str {
        "circular-group-rejection"
    }

    fn adr(&self) -> &str {
        "ADR-037"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Attempt to create group A containing group B
        let group_a = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "group",
            "name": "group-a",
            "members": ["group-b"],
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", group_a).await {
            return ScenarioResult::Skip {
                reason: format!("group creation endpoint not available: {}", e),
            };
        }

        // 2. Attempt to create group B containing group A (circular)
        let group_b = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "group",
            "name": "group-b",
            "members": ["group-a"],
        });

        match assertions::http_post(ctx, "/api/commands", group_b).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                // Circular membership must be rejected — expect 400 or 409
                if status == 400 || status == 409 || status == 422 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if status == 404 || status == 501 {
                    ScenarioResult::Skip {
                        reason: "group membership cycle detection not implemented yet".to_string(),
                    }
                } else {
                    // If it was accepted, check the event log for rejection
                    let ask_query = r#"
                        PREFIX picloud: <https://picloud.local/ontology#>
                        ASK {
                            ?event a picloud:ResourceFailed ;
                                   picloud:reason ?reason .
                            FILTER(CONTAINS(STR(?reason), "circular"))
                        }
                    "#;
                    match assertions::sparql_query(ctx, ask_query).await {
                        Ok(body) => {
                            let json: serde_json::Value =
                                serde_json::from_str(&body).unwrap_or_default();
                            if json.get("boolean").and_then(|v| v.as_bool()) == Some(true) {
                                ScenarioResult::Pass {
                                    duration: start.elapsed(),
                                }
                            } else {
                                ScenarioResult::Fail {
                                    duration: start.elapsed(),
                                    reason: "circular group membership was not rejected"
                                        .to_string(),
                                }
                            }
                        }
                        Err(e) => ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "could not verify circular group rejection: {}",
                                e
                            ),
                        },
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST circular group command: {}", e),
            },
        }
    }
}
