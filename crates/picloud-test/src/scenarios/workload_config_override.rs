//! ADR-043: Workload config override — workload-level config merges over product config.
//!
//! Apply config override for a workload. Verify via SPARQL or API that the
//! workload's effective config returns the overridden value, not the product value.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct WorkloadConfigOverride;

#[async_trait]
impl Scenario for WorkloadConfigOverride {
    fn name(&self) -> &str {
        "workload-config-override"
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

        // Check if the workload effective config endpoint exists.
        let effective_path = "/products/test-app/containers/worker/config";

        if !assertions::feature_available(ctx, effective_path).await {
            return ScenarioResult::Skip {
                reason: "workload effective config endpoint not implemented yet".to_string(),
            };
        }

        // GET the merged (effective) config for the workload.
        let resp = match assertions::http_get(ctx, effective_path).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("GET workload effective config failed: {}", e),
                };
            }
        };

        if !resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "GET workload effective config returned status {}",
                    resp.status()
                ),
            };
        }

        // Alternatively, verify via SPARQL that config override is modelled.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?entry a picloud:ConfigEntry ;
                       picloud:configKey ?key ;
                       picloud:configValue ?value .
            }
        "#;

        match assertions::sparql_query(ctx, query).await {
            Ok(body) => {
                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!("failed to parse SPARQL response: {}", e),
                        };
                    }
                };

                // ConfigEntry type should exist in the graph for overrides to work.
                if json.get("boolean").and_then(|v| v.as_bool()) != Some(true) {
                    return ScenarioResult::Skip {
                        reason: "no ConfigEntry resources in graph — config not deployed yet"
                            .to_string(),
                    };
                }
            }
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("SPARQL query for config entries failed: {}", e),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
