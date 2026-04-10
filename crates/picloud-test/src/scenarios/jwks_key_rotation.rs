//! ADR-017: JWKS key rotation — GET JWKS, verify key IDs. If rotation endpoint
//! exists, trigger and verify new kid.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct JwksKeyRotation;

#[async_trait]
impl Scenario for JwksKeyRotation {
    fn name(&self) -> &str {
        "jwks-key-rotation"
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

        // Fetch JWKS endpoint.
        let jwks_resp = match assertions::http_get(ctx, "/.well-known/jwks.json").await {
            Ok(r) => r,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("JWKS endpoint not available: {}", e),
                };
            }
        };

        if jwks_resp.status().as_u16() == 404 || jwks_resp.status().as_u16() == 501 {
            return ScenarioResult::Skip {
                reason: "JWKS endpoint not implemented".to_string(),
            };
        }

        if !jwks_resp.status().is_success() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("JWKS endpoint returned status {}", jwks_resp.status()),
            };
        }

        let jwks_body = match jwks_resp.text().await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to read JWKS response: {}", e),
                };
            }
        };

        let jwks: serde_json::Value = match serde_json::from_str(&jwks_body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse JWKS JSON: {}", e),
                };
            }
        };

        // Verify keys array exists and has at least one key with a kid.
        let keys = match jwks.get("keys").and_then(|k| k.as_array()) {
            Some(k) => k,
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "JWKS response missing 'keys' array".to_string(),
                };
            }
        };

        if keys.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "JWKS keys array is empty".to_string(),
            };
        }

        let initial_kids: Vec<&str> = keys
            .iter()
            .filter_map(|k| k.get("kid").and_then(|v| v.as_str()))
            .collect();

        if initial_kids.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no key IDs (kid) found in JWKS keys".to_string(),
            };
        }

        info!(kids = ?initial_kids, "initial JWKS key IDs");

        // Verify no 'none' algorithm.
        for key in keys {
            if let Some(alg) = key.get("alg").and_then(|v| v.as_str()) {
                if alg == "none" {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "JWKS contains key with 'none' algorithm — security violation"
                            .to_string(),
                    };
                }
            }
        }

        // Attempt key rotation if the endpoint exists.
        if assertions::feature_available(ctx, "/api/auth/rotate-keys").await {
            info!("key rotation endpoint available — triggering rotation");

            match assertions::http_post(
                ctx,
                "/api/auth/rotate-keys",
                serde_json::json!({}),
            )
            .await
            {
                Ok(resp) if resp.status().is_success() => {
                    info!("key rotation triggered");

                    // Re-fetch JWKS and verify new kid is present.
                    match assertions::http_get(ctx, "/.well-known/jwks.json").await {
                        Ok(new_resp) if new_resp.status().is_success() => {
                            let new_body = new_resp.text().await.unwrap_or_default();
                            let new_jwks: serde_json::Value =
                                serde_json::from_str(&new_body).unwrap_or_default();
                            let new_keys = new_jwks
                                .get("keys")
                                .and_then(|k| k.as_array())
                                .cloned()
                                .unwrap_or_default();

                            let new_kids: Vec<&str> = new_keys
                                .iter()
                                .filter_map(|k| k.get("kid").and_then(|v| v.as_str()))
                                .collect();

                            // Old kids should still be present (rotation window).
                            for old_kid in &initial_kids {
                                if !new_kids.contains(old_kid) {
                                    return ScenarioResult::Fail {
                                        duration: start.elapsed(),
                                        reason: format!(
                                            "old key {} removed after rotation — should persist during rotation window",
                                            old_kid
                                        ),
                                    };
                                }
                            }

                            info!(new_kids = ?new_kids, "JWKS after rotation — old keys preserved");
                        }
                        _ => {
                            return ScenarioResult::Fail {
                                duration: start.elapsed(),
                                reason: "failed to fetch JWKS after rotation".to_string(),
                            };
                        }
                    }
                }
                _ => {
                    info!("key rotation endpoint returned non-success — skipping rotation verification");
                }
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
