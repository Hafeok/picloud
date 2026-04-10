//! ADR-024: Intent translated to implementation — apply a volume with storage
//! intent and query SPARQL for implementation details.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct IntentTranslatedToImplementation;

#[async_trait]
impl Scenario for IntentTranslatedToImplementation {
    fn name(&self) -> &str {
        "intent-translated-to-implementation"
    }

    fn adr(&self) -> &str {
        "ADR-024"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Apply a volume with storage intent.
        let test_product = format!("test-intent-{}", uuid::Uuid::new_v4().as_simple());
        let command = serde_json::json!({
            "type": "ResourceDeclared",
            "product": test_product,
            "resource_type": "volume",
            "name": "data-vol",
            "durability": "full-replication",
            "size_gb": 10,
        });

        if let Err(e) = assertions::http_post(ctx, "/api/commands", command).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply volume: {}", e),
            };
        }

        // Query for implementation details.
        let vol_iri = format!(
            "https://picloud.local/products/{}/volumes/data-vol",
            test_product
        );

        let impl_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <{}> picloud:replicationFactor ?factor .
            }}
            "#,
            vol_iri
        );

        match assertions::wait_for_sparql(ctx, &impl_query, Duration::from_secs(15)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "volume implementation details not projected: {}",
                    e
                ),
            },
        }
    }
}
