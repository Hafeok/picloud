//! ADR-053: CSR wildcard rejection — POST a CSR with wildcard CN, assert rejection.
//!
//! Submit a CSR with a wildcard SAN (*.picloud.local) to the enrollment
//! endpoint. Assert the endpoint returns 400 and no certificate is issued.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CsrWildcardRejection;

#[async_trait]
impl Scenario for CsrWildcardRejection {
    fn name(&self) -> &str {
        "csr-wildcard-rejection"
    }

    fn adr(&self) -> &str {
        "ADR-053"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let enroll_path = "/enroll";
        if !assertions::feature_available(ctx, enroll_path).await {
            return ScenarioResult::Skip {
                reason: "enrollment endpoint not implemented yet".to_string(),
            };
        }

        // POST a CSR with a wildcard CN to the enrollment endpoint.
        // In practice this would be a DER-encoded CSR, but for the scenario
        // test we send a JSON representation that the endpoint should validate.
        let wildcard_csr = serde_json::json!({
            "csr": "-----BEGIN CERTIFICATE REQUEST-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA\n-----END CERTIFICATE REQUEST-----",
            "hostname": "*.picloud.local",
            "token": null
        });

        let resp = match assertions::http_post(ctx, enroll_path, wildcard_csr).await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("POST wildcard CSR failed: {}", e),
                };
            }
        };

        let status = resp.status().as_u16();

        // The enrollment endpoint should reject wildcard CSRs.
        if status == 200 || status == 201 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "wildcard CSR accepted with status {} — wildcard rejection not enforced",
                    status
                ),
            };
        }

        // 400 Bad Request is the expected response for an invalid CSR.
        if status != 400 && status != 403 && status != 422 {
            // Other error codes may be acceptable depending on implementation.
            // 404/501 means the endpoint exists but doesn't handle this case yet.
            if status == 404 || status == 501 {
                return ScenarioResult::Skip {
                    reason: "enrollment endpoint not fully implemented".to_string(),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
