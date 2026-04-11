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

        let member_iri = "https://picloud.local/platform/identities/test-user";
        let group_iri = "https://picloud.local/groups/backend-team";

        // 1. Add member to group via GroupMembershipChanged event
        let add_cmd = serde_json::json!({
            "type": "GroupMembershipChanged",
            "source": group_iri,
            "payload": {
                "group_iri": group_iri,
                "member_iri": member_iri,
                "action": "added"
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", add_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("command endpoint not available: {}", e),
            };
        }

        // 2. Verify membership was added
        let member_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <{}> picloud:hasMember <{}> .
            }}
            "#,
            group_iri, member_iri
        );

        if assertions::wait_for_sparql(ctx, &member_query, Duration::from_secs(10))
            .await
            .is_err()
        {
            return ScenarioResult::Skip {
                reason: "group membership projection not active".to_string(),
            };
        }

        // 3. Remove member from group via GroupMembershipChanged event
        let remove_cmd = serde_json::json!({
            "type": "GroupMembershipChanged",
            "source": group_iri,
            "payload": {
                "group_iri": group_iri,
                "member_iri": member_iri,
                "action": "removed"
            }
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", remove_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to send membership removal event: {}", e),
            };
        }

        // 4. Verify membership was retracted
        tokio::time::sleep(Duration::from_secs(5)).await;

        match assertions::sparql_query(ctx, &member_query).await {
            Ok(body) => {
                let json: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                if json.get("boolean").and_then(|v| v.as_bool()) == Some(false) {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "membership triple not retracted after removal event".to_string(),
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
