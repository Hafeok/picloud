//! ADR-017: OIDC authorization code flow — GET /auth/authorize with code flow
//! params, assert redirect response.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OidcAuthorizationCode;

#[async_trait]
impl Scenario for OidcAuthorizationCode {
    fn name(&self) -> &str {
        "oidc-authorization-code"
    }

    fn adr(&self) -> &str {
        "ADR-017"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Check OIDC discovery endpoint first.
        let discovery_resp =
            match assertions::http_get(ctx, "/.well-known/openid-configuration").await {
                Ok(r) => r,
                Err(e) => {
                    return ScenarioResult::Skip {
                        reason: format!("OIDC discovery endpoint unavailable: {}", e),
                    };
                }
            };

        if discovery_resp.status().as_u16() == 404 || discovery_resp.status().as_u16() == 501 {
            return ScenarioResult::Skip {
                reason: "OIDC discovery endpoint not implemented".to_string(),
            };
        }

        if !discovery_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "OIDC discovery returned status {}",
                    discovery_resp.status()
                ),
            };
        }

        let discovery_body = match discovery_resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read discovery response: {}", e),
                };
            }
        };

        let discovery: serde_json::Value = match serde_json::from_str(&discovery_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse discovery JSON: {}", e),
                };
            }
        };

        // Verify required OIDC discovery fields.
        let required_fields = [
            "issuer",
            "authorization_endpoint",
            "token_endpoint",
            "jwks_uri",
        ];
        for field in &required_fields {
            if discovery.get(field).is_none() {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("OIDC discovery missing required field: {}", field),
                };
            }
        }

        info!("OIDC discovery fields verified");

        // Build authorization code request — use a non-following client.
        let auth_url = format!(
            "{}/auth/authorize?response_type=code&client_id=test-client&redirect_uri=https://localhost/callback&scope=openid&state=test-state",
            ctx.config.base_url()
        );

        let no_redirect_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| ctx.http_client.clone());

        match no_redirect_client.get(&auth_url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                // Expect a redirect (302/303) or a login page (200).
                if status == 302 || status == 303 || status == 200 {
                    info!(status = status, "authorization endpoint responded correctly");
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if status == 404 || status == 501 {
                    ScenarioResult::Skip {
                        reason: "authorization endpoint not implemented".to_string(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "authorization endpoint returned unexpected status {}",
                            status
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("authorization request failed: {}", e),
            },
        }
    }
}
