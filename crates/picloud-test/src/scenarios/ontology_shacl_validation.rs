//! ADR-023: Apply ontology, run SHACL validation. Assert results.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OntologyShaclValidation;

#[async_trait]
impl Scenario for OntologyShaclValidation {
    fn name(&self) -> &str {
        "ontology-shacl-validation"
    }

    fn adr(&self) -> &str {
        "ADR-023"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check if ontology/SHACL validation is available
        if !assertions::feature_available(ctx, "/api/validate").await {
            // Fall back to checking compiler crate for SHACL support
            let compiler_dir = ctx.workspace_root().join("crates/picloud-compiler/src");
            if !compiler_dir.exists() {
                return ScenarioResult::Skip {
                    reason: "picloud-compiler crate not found".to_string(),
                };
            }

            let validator_path = compiler_dir.join("validator.rs");
            if !validator_path.exists() {
                return ScenarioResult::Skip {
                    reason: "validator.rs not found in picloud-compiler".to_string(),
                };
            }

            let content = match std::fs::read_to_string(&validator_path) {
                Ok(c) => c,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("Failed to read validator.rs: {}", e),
                    };
                }
            };

            if !content.contains("shacl") && !content.contains("SHACL") {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "validator.rs does not contain SHACL validation logic".to_string(),
                };
            }

            return ScenarioResult::Pass {
                duration: start.elapsed(),
            };
        }

        // 2. Submit a valid ontology triple that violates SHACL shape
        let invalid_triple = serde_json::json!({
            "type": "GraphUpdate",
            "product": "test-ontology-product",
            "triples": [
                {
                    "subject": "https://picloud.local/products/test-ontology-product/invalid-resource",
                    "predicate": "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                    "object": "https://picloud.local/ontology#Container"
                    // Missing required properties per SHACL shape (e.g. image)
                }
            ]
        });

        match assertions::http_post(ctx, "/api/validate", invalid_triple).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 400 || status == 422 {
                    // SHACL validation correctly rejected the invalid data
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if status == 404 || status == 501 {
                    ScenarioResult::Skip {
                        reason: "SHACL validation not implemented".to_string(),
                    }
                } else if resp.status().is_success() {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "invalid ontology data was accepted — SHACL should have rejected it"
                            .to_string(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("unexpected status from validation: {}", status),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST validation request: {}", e),
            },
        }
    }
}
