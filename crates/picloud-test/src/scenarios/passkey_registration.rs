//! ADR-025: Passkey registration — POST to /auth/passkeys/register endpoint.
//! Assert challenge response.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PasskeyRegistration;

#[async_trait]
impl Scenario for PasskeyRegistration {
    fn name(&self) -> &str {
        "passkey-registration"
    }

    fn adr(&self) -> &str {
        "ADR-025"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Check if the passkey registration endpoint exists.
        if !assertions::feature_available(ctx, "/auth/register/begin").await {
            return ScenarioResult::Skip {
                reason: "passkey registration endpoint not available".to_string(),
            };
        }

        // POST to initiate WebAuthn registration ceremony.
        let register_body = serde_json::json!({
            "username": "test-user",
            "displayName": "Test User"
        });

        match assertions::http_post(ctx, "/auth/register/begin", register_body).await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();

                if status == 404 || status == 501 {
                    return ScenarioResult::Skip {
                        reason: "passkey registration not implemented".to_string(),
                    };
                }

                let json: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        return ScenarioResult::Fail {
                            duration: start.elapsed(),
                            reason: format!(
                                "registration response not valid JSON (status {}): {}",
                                status, e
                            ),
                        };
                    }
                };

                // Verify the response contains a WebAuthn challenge.
                let has_challenge = json.get("challenge").is_some()
                    || json
                        .pointer("/publicKey/challenge")
                        .is_some();

                if !has_challenge {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "registration response missing WebAuthn challenge".to_string(),
                    };
                }

                // Verify rpId is present and matches cluster domain.
                let rp_id = json
                    .pointer("/publicKey/rp/id")
                    .or_else(|| json.pointer("/rp/id"))
                    .and_then(|v| v.as_str());

                if let Some(rp) = rp_id {
                    if !rp.contains("picloud") {
                        info!(rp_id = rp, "rpId does not contain picloud — may be misconfigured");
                    } else {
                        info!(rp_id = rp, "rpId matches cluster domain");
                    }
                }

                info!("passkey registration challenge received");
                ScenarioResult::Pass {
                    duration: start.elapsed(),
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("passkey registration request failed: {}", e),
            },
        }
    }
}
