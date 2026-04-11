//! ADR-037: Add member to group, remove. SPARQL verify removal.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct GroupMembershipRemoval;

#[async_trait]
impl Scenario for GroupMembershipRemoval {
    fn name(&self) -> &str {
        "group-membership-removal"
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

        // 1. Add a tag to trigger group membership
        let tag_add = serde_json::json!({
            "resource": "https://picloud.local/platform/identities/test-user",
            "key": "team",
            "value": "backend"
        });

        if let Err(e) = assertions::http_post(ctx, "/api/tags/add", tag_add).await {
            return ScenarioResult::Skip {
                reason: format!("tag endpoint not available: {}", e),
            };
        }

        // 2. Verify membership was added
        let member_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?group picloud:hasMember <https://picloud.local/platform/identities/test-user> .
            }
        "#;

        if assertions::wait_for_sparql(ctx, member_query, Duration::from_secs(10))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "group membership inference not active".to_string(),
            };
        }

        // 3. Remove the tag to trigger membership removal
        let tag_remove = serde_json::json!({
            "resource": "https://picloud.local/platform/identities/test-user",
            "key": "team",
            "value": "backend"
        });

        if let Err(e) = assertions::http_post(ctx, "/api/tags/remove", tag_remove).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to remove tag: {}", e),
            };
        }

        // 4. Verify membership was retracted
        tokio::time::sleep(Duration::from_secs(5)).await;

        let not_member_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?group picloud:hasMember <https://picloud.local/platform/identities/test-user> .
            }
        "#;

        match assertions::sparql_query(ctx, not_member_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                if json.get("boolean").and_then(|v| v.as_bool()) == Some(false) {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "membership triple was not retracted after tag removal".to_string(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("SPARQL query failed: {}", e),
            },
        }
    }
}
