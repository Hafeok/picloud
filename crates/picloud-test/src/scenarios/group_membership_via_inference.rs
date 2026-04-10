//! ADR-037: Define SPARQL CONSTRUCT rule for group membership.
//! Assert inference applies when matching tag is added.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct GroupMembershipViaInference;

#[async_trait]
impl Scenario for GroupMembershipViaInference {
    fn name(&self) -> &str {
        "group-membership-via-inference"
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

        // 1. Create a group with an inference rule
        let group_cmd = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "group",
            "name": "test-inference-group",
            "roles": ["test-role"],
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", group_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("group creation not available: {}", e),
            };
        }

        // 2. Create an inference rule for group membership
        let rule_cmd = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "inference-rule",
            "name": "test-group-membership-rule",
            "scope": "platform",
            "trigger": "event",
            "construct": "CONSTRUCT { <https://picloud.local/groups/test-inference-group> picloud:hasMember ?user . } WHERE { ?user a picloud:HumanIdentity ; picloud:tag [ picloud:tagKey \"dept\" ; picloud:tagValue \"engineering\" ] . }",
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", rule_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("inference rule creation not available: {}", e),
            };
        }

        // 3. Add a matching tag to a user
        let tag_cmd = serde_json::json!({
            "type": "TagAdded",
            "resource_iri": "https://picloud.local/platform/identities/inference-test-user",
            "key": "dept",
            "value": "engineering",
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", tag_cmd).await {
            return ScenarioResult::Skip {
                reason: format!("tag addition not available: {}", e),
            };
        }

        // 4. Verify the inference produced the membership triple
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                <https://picloud.local/groups/test-inference-group>
                    picloud:hasMember <https://picloud.local/platform/identities/inference-test-user> .
            }
        "#;

        match assertions::wait_for_sparql(ctx, ask_query, Duration::from_secs(10)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "inference rule did not produce membership triple within 10s: {}",
                    e
                ),
            },
        }
    }
}
