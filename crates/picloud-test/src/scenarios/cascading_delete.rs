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

        if !assertions::commands_available(ctx).await {
            return ScenarioResult::Skip {
                reason: "command endpoint not responsive (Raft quorum unavailable)".to_string(),
            };
        }

        // Apply a product with child resources.
        let test_name = format!("test-cascade-{}", uuid::Uuid::new_v4().as_simple());
        let resources = vec![
            serde_json::json!({
                "type": "product",
                "name": test_name,
                "version": "1.0.0"
            }),
            serde_json::json!({
                "type": "container",
                "name": "api-server",
                "product": test_name,
                "image": "test:1.0"
            }),
            serde_json::json!({
                "type": "container",
                "name": "worker",
                "product": test_name,
                "image": "test:1.0"
            }),
            serde_json::json!({
                "type": "volume",
                "name": "data-store",
                "product": test_name,
                "size_gb": 10
            }),
        ];

        if let Err(e) = assertions::apply_resources(ctx, resources).await {
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
        let delete_payload = serde_json::json!({
            "type": "product",
            "name": test_name,
        });

        if let Err(e) = assertions::http_post(ctx, "/api/delete", delete_payload).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to delete product: {}", e),
            };
        }

        // Poll until child resources are gone (projection needs time to cascade).
        let no_children_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                FILTER NOT EXISTS {{
                    ?resource picloud:belongsTo <https://picloud.local/products/{}> .
                }}
            }}
            "#,
            test_name
        );

        match assertions::wait_for_sparql(ctx, &no_children_query, Duration::from_secs(15)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(_) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "child resources still present after product deletion".to_string(),
            },
        }
    }
}
