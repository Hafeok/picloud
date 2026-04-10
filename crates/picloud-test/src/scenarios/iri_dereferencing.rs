//! ADR-029: IRI dereferencing — GET a known resource IRI, assert 200 with
//! JSON-LD containing @id matching the IRI.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct IriDereferencing;

#[async_trait]
impl Scenario for IriDereferencing {
    fn name(&self) -> &str {
        "iri-dereferencing"
    }

    fn adr(&self) -> &str {
        "ADR-029"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Find a known resource IRI via SPARQL.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?s WHERE { ?s a picloud:Node } LIMIT 1
        "#;

        let resource_iri = match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(_) => {
                        return ScenarioResult::Skip {
                            reason: "no resources found to dereference".to_string(),
                        };
                    }
                };
                match json
                    .pointer("/results/bindings/0/s/value")
                    .and_then(|v| v.as_str())
                {
                    Some(iri) => iri.to_string(),
                    None => {
                        return ScenarioResult::Skip {
                            reason: "no resources found to dereference".to_string(),
                        };
                    }
                }
            }
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "cannot query for resources".to_string(),
                };
            }
        };

        // Dereference the IRI with Accept: application/ld+json.
        let resp = match ctx
            .http_client
            .get(&resource_iri)
            .header("Accept", "application/ld+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to dereference IRI {}: {}", resource_iri, e),
                };
            }
        };

        let status = resp.status().as_u16();
        if status != 200 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "IRI {} returned {} (expected 200)",
                    resource_iri, status
                ),
            };
        }

        let body = resp.text().await.unwrap_or_default();
        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("response is not valid JSON: {}", e),
                };
            }
        };

        // Check for @id matching the requested IRI.
        let id = json
            .get("@id")
            .or_else(|| json.get("id"))
            .and_then(|v| v.as_str());

        match id {
            Some(found_id) if found_id == resource_iri => ScenarioResult::Pass {
                duration: start.elapsed(),
            },
            Some(found_id) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "@id mismatch: expected {}, got {}",
                    resource_iri, found_id
                ),
            },
            None => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "response missing @id or id field".to_string(),
            },
        }
    }
}
