//! ADR-036: Tag remove — apply a tagged resource, remove the tag, verify
//! via SPARQL ASK that the tag is gone.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct TagRemove;

#[async_trait]
impl Scenario for TagRemove {
    fn name(&self) -> &str {
        "tag-remove"
    }

    fn adr(&self) -> &str {
        "ADR-036"
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

        let test_product = format!("test-tagrem-{}", uuid::Uuid::new_v4().as_simple());

        // Apply the product resource.
        if let Err(e) = assertions::apply_product_and_wait(ctx, &test_product, "1.0.0").await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply product: {}", e),
            };
        }

        // Add a tag to the product.
        let product_iri = format!("https://picloud.local/products/{}", test_product);
        let add_tag_cmd = serde_json::json!({
            "resource": product_iri,
            "key": "environment",
            "value": "staging"
        });

        if let Err(e) = assertions::http_post(ctx, "/api/tags/add", add_tag_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to add tag: {}", e),
            };
        }

        // Wait for the tag to appear.
        let tag_ask = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <{}> picloud:tag [
                    picloud:tagKey "environment" ;
                    picloud:tagValue "staging"
                ] .
            }}
            "#,
            product_iri
        );

        let _ = assertions::wait_for_sparql(ctx, &tag_ask, Duration::from_secs(10)).await;

        // Remove the tag.
        let remove_cmd = serde_json::json!({
            "resource": product_iri,
            "key": "environment",
            "value": "staging"
        });

        if let Err(e) = assertions::http_post(ctx, "/api/tags/remove", remove_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to remove tag: {}", e),
            };
        }

        // Wait and verify tag is gone.
        tokio::time::sleep(Duration::from_secs(5)).await;

        match assertions::sparql_query(ctx, &tag_ask).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse response: {}", e),
                        };
                    }
                };

                let tag_present =
                    json.get("boolean").and_then(|v| v.as_bool()).unwrap_or(true);

                if tag_present {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "tag still present after removal".to_string(),
                    }
                } else {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("SPARQL query after tag removal failed: {}", e),
            },
        }
    }
}
