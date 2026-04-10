//! ADR-016: Cascading delete — apply a product with resources, DELETE the
//! product, then verify child resources are gone from the graph.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CascadingDelete;

#[async_trait]
impl Scenario for CascadingDelete {
    fn name(&self) -> &str {
        "cascading-delete"
    }

    fn adr(&self) -> &str {
        "ADR-016"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Apply a product with child resources.
        let test_name = format!("test-cascade-{}", uuid::Uuid::new_v4().as_simple());
        let command = serde_json::json!({
            "type": "ResourceDeclared",
            "product": test_name,
            "resource_type": "product",
            "name": test_name,
            "resources": [
                { "type": "container", "name": "api-server" },
                { "type": "container", "name": "worker" },
                { "type": "volume", "name": "data-store" },
            ]
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", command).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply product: {}", e),
            };
        }

        // Wait for product to appear.
        let ask_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <https://picloud.local/products/{}> a picloud:Product .
            }}
            "#,
            test_name
        );

        if let Err(e) =
            assertions::wait_for_sparql(ctx, &ask_query, Duration::from_secs(10)).await
        {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("product not projected before delete: {}", e),
            };
        }

        // DELETE the product.
        let delete_cmd = serde_json::json!({
            "type": "ProductDeleted",
            "product": test_name,
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", delete_cmd).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to delete product: {}", e),
            };
        }

        // Wait and verify child resources are gone.
        tokio::time::sleep(Duration::from_secs(5)).await;

        let child_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                ?resource picloud:belongsTo <https://picloud.local/products/{}> .
            }}
            "#,
            test_name
        );

        match assertions::sparql_query(ctx, &child_query).await {
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

                let has_children =
                    json.get("boolean").and_then(|v| v.as_bool()).unwrap_or(true);

                if has_children {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "child resources still present after product deletion"
                            .to_string(),
                    }
                } else {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("SPARQL query for children failed: {}", e),
            },
        }
    }
}
