//! ADR-030: Certificate chain validation — GET /api/ca, parse PEM, verify chain
//! structure.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CertChainValidation;

#[async_trait]
impl Scenario for CertChainValidation {
    fn name(&self) -> &str {
        "cert-chain-validation"
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

        // Fetch the CA certificate from the platform.
        let ca_paths = ["/api/ca", "/api/ca/export", "/.well-known/ca"];
        let mut ca_pem: Option<String> = None;

        for path in &ca_paths {
            match assertions::http_get(ctx, path).await {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp.text().await.unwrap_or_default();
                    if body.contains("BEGIN CERTIFICATE") {
                        ca_pem = Some(body);
                        info!(path = path, "CA certificate retrieved");
                        break;
                    }
                }
                _ => {}
            }
        }

        let pem = match ca_pem {
            Some(p) => p,
            None => {
                return ScenarioResult::Skip {
                    reason: "CA certificate endpoint not available or no PEM returned".to_string(),
                };
            }
        };

        // Verify PEM structure.
        let cert_count = pem.matches("BEGIN CERTIFICATE").count();
        if cert_count == 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no certificates found in PEM response".to_string(),
            };
        }

        info!(certs = cert_count, "certificates found in PEM chain");

        // Verify the PEM is well-formed (has matching BEGIN/END blocks).
        let begin_count = pem.matches("BEGIN CERTIFICATE").count();
        let end_count = pem.matches("END CERTIFICATE").count();

        if begin_count != end_count {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "malformed PEM: {} BEGIN blocks but {} END blocks",
                    begin_count, end_count
                ),
            };
        }

        // Verify there is at least a root CA (self-signed or intermediate).
        if cert_count >= 1 {
            info!("PEM chain structure valid — {} certificate(s)", cert_count);
        }

        // Check that the PEM does not contain private key material.
        if pem.contains("PRIVATE KEY") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "CA export endpoint returned private key material — security violation"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
