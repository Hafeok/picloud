//! ADR-029: Content negotiation — GET a resource IRI with different Accept
//! headers and compare Content-Type in responses.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ContentNegotiation;

#[async_trait]
impl Scenario for ContentNegotiation {
    fn name(&self) -> &str {
        "content-negotiation"
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

        let mut issues: Vec<String> = Vec::new();

        // Test content negotiation on the cluster root IRI.
        let test_path = "/";

        // Request with Accept: text/turtle.
        let turtle_url = format!("{}{}", ctx.config.base_url(), test_path);
        match ctx
            .http_client
            .get(&turtle_url)
            .header("Accept", "text/turtle")
            .send()
            .await
        {
            Ok(resp) => {
                let ct = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let status = resp.status().as_u16();
                if status == 200 && !ct.contains("turtle") {
                    issues.push(format!(
                        "Accept: text/turtle returned Content-Type: {} (expected turtle)",
                        ct
                    ));
                }
            }
            Err(e) => issues.push(format!("turtle request failed: {}", e)),
        }

        // Request with Accept: application/ld+json.
        match ctx
            .http_client
            .get(&turtle_url)
            .header("Accept", "application/ld+json")
            .send()
            .await
        {
            Ok(resp) => {
                let ct = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let status = resp.status().as_u16();
                if status == 200 && !ct.contains("json") {
                    issues.push(format!(
                        "Accept: application/ld+json returned Content-Type: {} (expected json)",
                        ct
                    ));
                }
            }
            Err(e) => issues.push(format!("JSON-LD request failed: {}", e)),
        }

        if issues.is_empty() {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "{} content negotiation issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
