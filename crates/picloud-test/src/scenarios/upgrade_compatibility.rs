//! ADR-055: Upgrade Compatibility — verify RDF graph integrity across upgrades.
//!
//! Stages a rolling upgrade from the current binary to the one specified by
//! `PICLOUD_TEST_BINARY`, then asserts:
//! - Triple count is identical before and after the upgrade.
//! - All resource IRIs remain dereferenceable (HTTP 200).

use std::path::PathBuf;
use std::time::Instant;

use async_trait::async_trait;
use tracing::{info, warn};

use crate::harness::assertions::sparql_query;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};
use crate::upgrade::{rolling_upgrade, UpgradeConfig};

pub struct UpgradeCompatibilityScenario;

const TRIPLE_COUNT_QUERY: &str = "SELECT (COUNT(*) AS ?count) WHERE { ?s ?p ?o }";

const RESOURCE_IRI_QUERY: &str = r#"
    PREFIX picloud: <https://picloud.local/ontology#>
    SELECT DISTINCT ?iri WHERE {
        ?iri a ?type .
        FILTER(STRSTARTS(STR(?iri), "https://picloud.local/"))
    }
"#;

/// Parse a SPARQL results JSON response and extract the count from the first binding.
fn parse_count(body: &str) -> Result<u64, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {}", e))?;
    let val = json
        .pointer("/results/bindings/0/count/value")
        .and_then(|v| v.as_str())
        .ok_or("missing /results/bindings/0/count/value")?;
    val.parse::<u64>()
        .map_err(|e| format!("count not a u64: {}", e))
}

/// Extract all IRI strings from a SPARQL results JSON response.
fn parse_iris(body: &str) -> Result<Vec<String>, String> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {}", e))?;
    let bindings = json
        .pointer("/results/bindings")
        .and_then(|v| v.as_array())
        .ok_or("missing /results/bindings")?;
    let mut iris = Vec::new();
    for binding in bindings {
        if let Some(iri) = binding
            .pointer("/iri/value")
            .and_then(|v| v.as_str())
        {
            iris.push(iri.to_string());
        }
    }
    Ok(iris)
}

#[async_trait]
impl Scenario for UpgradeCompatibilityScenario {
    fn name(&self) -> &str {
        "upgrade-compatibility"
    }

    fn adr(&self) -> &str {
        "ADR-055"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Check for the test binary env var.
        let binary_path = match std::env::var("PICLOUD_TEST_BINARY") {
            Ok(p) => PathBuf::from(p),
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "PICLOUD_TEST_BINARY not set".to_string(),
                };
            }
        };

        if !binary_path.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("binary not found at {}", binary_path.display()),
            };
        }

        // Step 1: Capture pre-upgrade triple count and resource IRIs.
        let pre_count = match sparql_query(ctx, TRIPLE_COUNT_QUERY).await {
            Ok(body) => match parse_count(&body) {
                Ok(c) => c,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("pre-upgrade triple count parse error: {}", e),
                    };
                }
            },
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("pre-upgrade SPARQL query failed: {}", e),
                };
            }
        };

        let pre_iris = match sparql_query(ctx, RESOURCE_IRI_QUERY).await {
            Ok(body) => match parse_iris(&body) {
                Ok(iris) => iris,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("pre-upgrade IRI parse error: {}", e),
                    };
                }
            },
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("pre-upgrade IRI query failed: {}", e),
                };
            }
        };

        info!(
            pre_count = pre_count,
            pre_iris = pre_iris.len(),
            "captured pre-upgrade state"
        );

        // Step 2: Perform the rolling upgrade.
        let upgrade_config = UpgradeConfig {
            binary_path,
            cluster_config: ctx.config.clone(),
        };

        let report = match rolling_upgrade(upgrade_config).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("rolling upgrade error: {}", e),
                };
            }
        };

        if !report.success {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "rolling upgrade reported failure".to_string(),
            };
        }

        // Step 3: Capture post-upgrade triple count.
        let post_count = match sparql_query(ctx, TRIPLE_COUNT_QUERY).await {
            Ok(body) => match parse_count(&body) {
                Ok(c) => c,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("post-upgrade triple count parse error: {}", e),
                    };
                }
            },
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("post-upgrade SPARQL query failed: {}", e),
                };
            }
        };

        if pre_count != post_count {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "triple count changed: {} before upgrade, {} after",
                    pre_count, post_count
                ),
            };
        }

        info!(
            triple_count = post_count,
            "triple count identical pre/post upgrade"
        );

        // Step 4: Verify all pre-upgrade resource IRIs are still dereferenceable.
        for iri in &pre_iris {
            // Convert the absolute IRI to a path relative to the cluster base URL.
            let path = if let Some(suffix) = iri.strip_prefix(&ctx.config.base_url()) {
                suffix.to_string()
            } else {
                // IRI uses picloud.local domain — build path from the IRI.
                match url::Url::parse(iri) {
                    Ok(parsed) => parsed.path().to_string(),
                    Err(_) => {
                        warn!(iri = iri.as_str(), "could not parse IRI, skipping");
                        continue;
                    }
                }
            };

            let url = format!("{}{}", ctx.config.base_url(), path);
            match ctx.http_client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "resource IRI {} returned {} after upgrade",
                            iri,
                            resp.status()
                        ),
                    };
                }
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "resource IRI {} unreachable after upgrade: {}",
                            iri, e
                        ),
                    };
                }
            }
        }

        info!(
            iris_checked = pre_iris.len(),
            "all resource IRIs dereferenceable after upgrade"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
