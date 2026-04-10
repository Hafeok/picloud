//! ADR-031: Schema IRI resolution — GET an event schema IRI and assert it
//! returns the schema definition.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SchemaIriResolution;

#[async_trait]
impl Scenario for SchemaIriResolution {
    fn name(&self) -> &str {
        "schema-iri-resolution"
    }

    fn adr(&self) -> &str {
        "ADR-031"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Try to resolve a known platform event schema IRI.
        let schema_paths = [
            "/schemas/events/ResourceReady/v1",
            "/schemas/events/ResourceDeclared/v1",
            "/schemas/events/NodeJoined/v1",
        ];

        for path in &schema_paths {
            match assertions::http_get(ctx, path).await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == 200 {
                        let body = resp.text().await.unwrap_or_default();

                        if body.trim().is_empty() {
                            return ScenarioResult::Fail {
                                duration: start.elapsed(),
                                reason: format!(
                                    "{} returned 200 but body is empty",
                                    path
                                ),
                            };
                        }

                        // Check that it looks like a schema (JSON Schema or SHACL).
                        let is_schema = body.contains("$schema")
                            || body.contains("properties")
                            || body.contains("@prefix")
                            || body.contains("sh:NodeShape")
                            || body.contains("type");

                        if is_schema {
                            return ScenarioResult::Pass {
                                duration: start.elapsed(),
                            };
                        }

                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "{} returned content but it does not appear to be a schema",
                                path
                            ),
                        };
                    }
                }
                Err(_) => continue,
            }
        }

        ScenarioResult::Fail {
            duration: start.elapsed(),
            reason: "no schema IRI resolved — tried ResourceReady, ResourceDeclared, NodeJoined"
                .to_string(),
        }
    }
}
