//! ADR-056: Shared Hardware Isolation — verify zero resource overlap between
//! production and staging clusters running on the same hardware.
//!
//! Queries SPARQL on both the production cluster (default port) and the staging
//! cluster (port 8443) for all resource IRIs, then asserts the intersection is
//! empty.

use std::collections::HashSet;
use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SharedHardwareIsolationScenario;

const RESOURCE_IRI_QUERY: &str = r#"
    PREFIX picloud: <https://picloud.local/ontology#>
    SELECT DISTINCT ?iri WHERE {
        ?iri a ?type .
        FILTER(STRSTARTS(STR(?iri), "https://"))
    }
"#;

/// Query SPARQL at the given base URL and return a set of all resource IRIs.
async fn collect_iris(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<HashSet<String>, String> {
    let url = format!("{}/graph", base_url);
    let resp = client
        .get(&url)
        .query(&[("query", RESOURCE_IRI_QUERY)])
        .header("Accept", "application/sparql-results+json")
        .send()
        .await
        .map_err(|e| format!("request to {} failed: {}", url, e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("SPARQL query to {} returned {}: {}", url, status, body));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("reading response body: {}", e))?;

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("invalid JSON: {}", e))?;

    let bindings = json
        .pointer("/results")
        .and_then(|v| v.as_array())
        .ok_or("missing /results")?;

    let mut iris = HashSet::new();
    for binding in bindings {
        if let Some(iri) = binding.pointer("/iri/value").and_then(|v| v.as_str()) {
            iris.insert(iri.to_string());
        }
    }
    Ok(iris)
}

#[async_trait]
impl Scenario for SharedHardwareIsolationScenario {
    fn name(&self) -> &str {
        "shared-hardware-isolation"
    }

    fn adr(&self) -> &str {
        "ADR-056"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Production cluster: use the configured base URL.
        let prod_url = ctx.config.base_url();

        // Staging cluster: derive from the first node IP on port 8443.
        let scheme = if ctx.config.cluster.tls { "https" } else { "http" };
        let staging_url = match ctx.config.first_node_ip() {
            Some(ip) => format!("{}://{}:8443", scheme, ip),
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "no nodes configured — cannot determine staging URL".to_string(),
                };
            }
        };

        // Collect production IRIs.
        let prod_iris = match collect_iris(&ctx.http_client, &prod_url).await {
            Ok(iris) => iris,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("production SPARQL failed: {}", e),
                };
            }
        };

        // Collect staging IRIs — skip gracefully if staging is not running.
        let staging_iris = match collect_iris(&ctx.http_client, &staging_url).await {
            Ok(iris) => iris,
            Err(e) => {
                let reason = format!("{}", e);
                if reason.contains("connection refused")
                    || reason.contains("Connection refused")
                    || reason.contains("connect error")
                    || reason.contains("tcp connect error")
                {
                    return ScenarioResult::Skip {
                        reason: format!(
                            "staging cluster not running at {} — {}",
                            staging_url, e
                        ),
                    };
                }
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("staging SPARQL failed: {}", e),
                };
            }
        };

        // Compute intersection.
        let overlap: Vec<&String> = prod_iris.intersection(&staging_iris).collect();

        if !overlap.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "cross-contamination detected: {} shared IRIs between production and staging. Examples: {:?}",
                    overlap.len(),
                    overlap.iter().take(5).collect::<Vec<_>>()
                ),
            };
        }

        info!(
            prod_iris = prod_iris.len(),
            staging_iris = staging_iris.len(),
            "zero overlap between production and staging resource IRIs"
        );

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
