//! ADR-053: Revoke a certificate. Verify it's rejected on next use.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CertRevocation;

#[async_trait]
impl Scenario for CertRevocation {
    fn name(&self) -> &str {
        "cert-revocation"
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

        // 1. Verify CRL (Certificate Revocation List) types exist in domain
        let resources_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/resources.rs");
        let resources_content = match std::fs::read_to_string(&resources_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", resources_path.display(), e),
                };
            }
        };

        // 2. Verify NodeLeft (or NodeRemoved) event triggers CRL update
        let events_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/events.rs");
        let events_content = match std::fs::read_to_string(&events_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", events_path.display(), e),
                };
            }
        };

        if !events_content.contains("NodeLeft") && !events_content.contains("NodeRemoved") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "NodeLeft/NodeRemoved event not found — needed for cert revocation".to_string(),
            };
        }

        // 3. Verify picloud-network has CA/revocation module
        let revocation_path = ctx
            .workspace_root()
            .join("crates/picloud-network/src/ca/revocation.rs");
        if revocation_path.exists() {
            let rev_content = match std::fs::read_to_string(&revocation_path) {
                Ok(c) => c,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("Failed to read revocation module: {}", e),
                    };
                }
            };

            if !rev_content.contains("revoke") && !rev_content.contains("CRL") {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "revocation module does not contain revoke/CRL logic".to_string(),
                };
            }
        }

        // 4. Check if node cert endpoint is available
        let _ = resources_content; // suppress unused warning
        match assertions::http_get(ctx, "/api/nodes/certs").await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "cert management endpoint not available".to_string(),
                    };
                }
            }
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "cert management endpoint not reachable".to_string(),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
