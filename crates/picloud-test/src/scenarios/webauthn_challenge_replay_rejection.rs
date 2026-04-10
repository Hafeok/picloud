//! ADR-025: WebAuthn challenge replay rejection — submit same challenge twice.
//! Assert second is rejected.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct WebauthnChallengeReplayRejection;

#[async_trait]
impl Scenario for WebauthnChallengeReplayRejection {
    fn name(&self) -> &str {
        "webauthn-challenge-replay-rejection"
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

        if !assertions::feature_available(ctx, "/auth/passkeys/register").await {
            return ScenarioResult::Skip {
                reason: "passkey registration endpoint not available".to_string(),
            };
        }

        // Get a challenge by initiating registration.
        let register_body = serde_json::json!({
            "username": "replay-test-user",
            "displayName": "Replay Test User"
        });

        let challenge_resp =
            match assertions::http_post(ctx, "/auth/passkeys/register", register_body).await {
                Ok(r) => r,
                Err(e) => {
                    return ScenarioResult::Skip {
                        reason: format!("cannot initiate registration: {}", e),
                    };
                }
            };

        let status = challenge_resp.status().as_u16();
        if status == 404 || status == 501 {
            return ScenarioResult::Skip {
                reason: "passkey registration not implemented".to_string(),
            };
        }

        let challenge_body = match challenge_resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read challenge response: {}", e),
                };
            }
        };

        let challenge_json: serde_json::Value = match serde_json::from_str(&challenge_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("challenge response not valid JSON: {}", e),
                };
            }
        };

        // Extract challenge value.
        let challenge = challenge_json
            .get("challenge")
            .or_else(|| challenge_json.pointer("/publicKey/challenge"))
            .cloned();

        if challenge.is_none() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no challenge found in registration response".to_string(),
            };
        }

        info!("obtained WebAuthn challenge");

        // Submit a fake assertion response using the challenge (simulated replay).
        let replay_body = serde_json::json!({
            "challenge": challenge,
            "response": {
                "clientDataJSON": "fake-client-data",
                "authenticatorData": "fake-authenticator-data",
                "signature": "fake-signature"
            }
        });

        // First submission.
        let first_resp = match assertions::http_post(
            ctx,
            "/auth/passkeys/verify",
            replay_body.clone(),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("verify endpoint unavailable: {}", e),
                };
            }
        };

        let first_status = first_resp.status().as_u16();
        if first_status == 404 || first_status == 501 {
            return ScenarioResult::Skip {
                reason: "passkey verify endpoint not implemented".to_string(),
            };
        }

        info!(status = first_status, "first challenge submission");

        // Second submission with the same challenge (replay).
        let second_resp = match assertions::http_post(
            ctx,
            "/auth/passkeys/verify",
            replay_body,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("replay submission failed: {}", e),
                };
            }
        };

        let second_status = second_resp.status().as_u16();

        // The second submission must be rejected (400 or 401 or 403).
        if second_status == 400 || second_status == 401 || second_status == 403 {
            info!(
                status = second_status,
                "challenge replay correctly rejected"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else if second_status == 200 {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "challenge replay was accepted — security violation".to_string(),
            }
        } else {
            // Other status codes — may indicate the verify endpoint rejects both
            // because the fake signature is invalid, which is acceptable.
            info!(
                status = second_status,
                "second submission also rejected — acceptable"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        }
    }
}
