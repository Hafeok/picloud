//! ADR-024: Storage intent full replication — apply a full-replication volume
//! and verify replication triples via SPARQL.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct StorageIntentFullReplication;

#[async_trait]
impl Scenario for StorageIntentFullReplication {
    fn name(&self) -> &str {
        "storage-intent-full-replication"
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

        if !assertions::commands_available(ctx).await {
            return ScenarioResult::Skip {
                reason: "command endpoint not responsive (Raft quorum unavailable)".to_string(),
            };
        }

        // Apply a product first, then a volume with full-replication durability.
        let test_product = format!("test-fullrep-{}", uuid::Uuid::new_v4().as_simple());
        let resources = vec![
            serde_json::json!({
                "type": "product",
                "name": test_product,
                "version": "1.0.0"
            }),
            serde_json::json!({
                "type": "volume",
                "name": "replicated-vol",
                "product": test_product,
                "size_gb": 5
            }),
        ];

        if let Err(e) = assertions::apply_resources(ctx, resources).await {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to apply volume: {}", e),
            };
        }

        // ASK for replication triples on the volume.
        let vol_iri = format!(
            "https://picloud.local/products/{}/volumes/replicated-vol",
            test_product
        );

        let ask_query = format!(
            r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {{
                <{}> picloud:durability "full-replication" .
            }}
            "#,
            vol_iri
        );

        match assertions::wait_for_sparql(ctx, &ask_query, Duration::from_secs(15)).await {
            Ok(()) => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "full-replication triples not found for volume: {}",
                    e
                ),
            },
        }
    }
}
