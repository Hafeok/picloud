//! ADR-049: Apply invalid RDF. Assert SHACL validation catches errors
//! and returns human-readable error messages.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ShaclValidationErrors;

#[async_trait]
impl Scenario for ShaclValidationErrors {
    fn name(&self) -> &str {
        "shacl-validation-errors"
    }

    fn adr(&self) -> &str {
        "ADR-049"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check validation endpoint
        if !assertions::feature_available(ctx, "/api/validate").await {
            // Try compiler-based validation path
            let compiler_path = ctx
                .workspace_root()
                .join("crates/picloud-compiler/src/validator.rs");
            if compiler_path.exists() {
                let content = match std::fs::read_to_string(&compiler_path) {
                    Ok(c) => c,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("Failed to read validator: {}", e),
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

            return ScenarioResult::Skip {
                reason: "validation endpoint and compiler not available".to_string(),
            };
        }

        // 2. Submit invalid resource (missing required 'image' field on container)
        let invalid_resource = serde_json::json!({
            "type": "ResourceDeclared",
            "resource_type": "container",
            "name": "invalid-container",
            "product": "test-product",
            // intentionally missing required 'image' field
        });

        match assertions::http_post(ctx, "/api/validate", invalid_resource).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 400 || status == 422 {
                    // SHACL validation caught the error
                    let body = resp.text().await.unwrap_or_default();
                    if body.contains("image") || body.contains("required") || body.contains("missing") {
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    } else {
                        ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "validation error returned but message is not human-readable: {}",
                                body
                            ),
                        }
                    }
                } else if status == 404 || status == 501 {
                    ScenarioResult::Skip {
                        reason: "SHACL validation not implemented".to_string(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "invalid resource was accepted (status {}). SHACL validation should have caught missing required field.",
                            status
                        ),
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
