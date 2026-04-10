//! ADR-043: Store secret via config API. Assert it's stored separately
//! from regular config (sensitive keys rejected in config store).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ConfigSecretSeparation;

#[async_trait]
impl Scenario for ConfigSecretSeparation {
    fn name(&self) -> &str {
        "config-secret-separation"
    }

    fn adr(&self) -> &str {
        "ADR-043"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // 1. Check config endpoint exists
        if !assertions::feature_available(ctx, "/products/test-product/config").await {
            return ScenarioResult::Skip {
                reason: "config endpoint not available".to_string(),
            };
        }

        // 2. Attempt to set a config entry with a sensitive key (e.g. "password")
        let secret_config = serde_json::json!({
            "key": "password",
            "value": "super-secret-value",
            "type": "string",
        });

        match assertions::http_post(ctx, "/products/test-product/config", secret_config).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                // Platform should reject sensitive keys in config store
                if status == 400 || status == 422 || status == 403 {
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if status == 404 || status == 501 {
                    ScenarioResult::Skip {
                        reason: "config secret separation not implemented yet".to_string(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "sensitive key 'password' was accepted in config store (status {}). Secrets must be stored separately.",
                            status
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to POST config entry: {}", e),
            },
        }
    }
}
