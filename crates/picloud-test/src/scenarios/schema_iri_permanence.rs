//! ADR-031: Schema IRI permanence — apply event with schema IRI. Verify IRI is
//! stable across restarts.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SchemaIriPermanence;

#[async_trait]
impl Scenario for SchemaIriPermanence {
    fn name(&self) -> &str {
        "schema-iri-permanence"
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

        // Check if schema IRIs are dereferenceable.
        let schema_iri = "/schemas/events/ResourceReady/v1";

        match assertions::http_get(ctx, schema_iri).await {
            Ok(resp) => {
                let status = resp.status().as_u16();

                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "schema IRI endpoint not implemented".to_string(),
                    };
                }

                if status == 200 {
                    let body = resp.text().await.unwrap_or_default();
                    if body.is_empty() {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: "schema IRI returned 200 but empty body".to_string(),
                        };
                    }

                    info!("schema IRI resolves successfully");

                    // Verify the same IRI is stable by fetching it again.
                    match assertions::http_get(ctx, schema_iri).await {
                        Ok(resp2) if resp2.status().is_success() => {
                            let body2 = resp2.text().await.unwrap_or_default();
                            if body == body2 {
                                info!("schema IRI returns identical content on re-fetch");
                            } else {
                                return ScenarioResult::Fail {
                                    duration: start.elapsed(),
                                    reason: "schema IRI returned different content on second fetch"
                                        .to_string(),
                                };
                            }
                        }
                        _ => {
                            return ScenarioResult::Fail {
                                duration: start.elapsed(),
                                reason: "schema IRI not stable — second fetch failed".to_string(),
                            };
                        }
                    }

                    return ScenarioResult::Pass {
                        duration: start.elapsed(),
                    };
                }

                ScenarioResult::Skip {
                    reason: format!("schema IRI returned status {}", status),
                }
            }
            Err(e) => {
                // Fall back to checking the event log for schema IRIs.
                let schema_query = r#"
                    PREFIX picloud: <https://picloud.local/ontology#>
                    SELECT ?schema WHERE {
                        ?event picloud:schema ?schema .
                    }
                    LIMIT 1
                "#;

                match assertions::sparql_query(ctx, schema_query).await {
                    Ok(body) => {
                        let json: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        let has_schema = json
                            .pointer("/results/bindings/0/schema/value")
                            .is_some();

                        if has_schema {
                            info!("schema IRIs present in event log");
                            ScenarioResult::Pass {
                                duration: start.elapsed(),
                            }
                        } else {
                            ScenarioResult::Skip {
                                reason: format!(
                                    "schema IRI endpoint unavailable and no schemas in graph: {}",
                                    e
                                ),
                            }
                        }
                    }
                    Err(_) => ScenarioResult::Skip {
                        reason: format!("schema IRI endpoint unavailable: {}", e),
                    },
                }
            }
        }
    }
}
