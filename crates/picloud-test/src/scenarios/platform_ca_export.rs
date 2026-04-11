//! ADR-030: Platform CA export — GET the CA certificate endpoint and assert
//! a PEM-encoded certificate in the response.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PlatformCaExport;

#[async_trait]
impl Scenario for PlatformCaExport {
    fn name(&self) -> &str {
        "platform-ca-export"
    }

    fn adr(&self) -> &str {
        "ADR-030"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Try known CA export endpoints.
        let ca_paths = ["/api/ca", "/ca", "/api/ca/certificate", "/api/enroll-node"];

        for path in &ca_paths {
            match assertions::http_get(ctx, path).await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status == 200 {
                        let body = resp.text().await.unwrap_or_default();

                        // Check for PEM-encoded certificate.
                        if body.contains("-----BEGIN CERTIFICATE-----") {
                            return ScenarioResult::Pass {
                                duration: start.elapsed(),
                            };
                        }

                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "{} returned 200 but body is not PEM-encoded",
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
            reason: "no CA export endpoint found at /api/ca, /ca, or /api/ca/certificate"
                .to_string(),
        }
    }
}
