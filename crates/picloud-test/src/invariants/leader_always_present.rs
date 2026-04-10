//! Invariant: Leader Always Present — at least one node must hold the
//! `picloud:hasRole picloud:Leader` role at all times.
//!
//! This invariant is used by the `rolling-upgrade-sequence` scenario (ADR-055)
//! to continuously validate quorum during upgrades, and can be composed into
//! any scenario that needs to verify leader availability.

use crate::harness::runner::TestContext;
use crate::invariants::InvariantResult;

pub struct LeaderAlwaysPresentInvariant;

const LEADER_QUERY: &str = r#"
    PREFIX picloud: <https://picloud.local/ontology#>
    SELECT ?node WHERE {
        ?node picloud:hasRole picloud:Leader .
    }
"#;

impl crate::invariants::Invariant for LeaderAlwaysPresentInvariant {
    fn name(&self) -> &str {
        "leader-always-present"
    }

    fn check(
        &self,
        ctx: &TestContext,
    ) -> impl std::future::Future<Output = InvariantResult> + Send {
        async move {
            let url = format!("{}/sparql", ctx.config.base_url());
            let resp = match ctx
                .http_client
                .post(&url)
                .header("Content-Type", "application/sparql-query")
                .header("Accept", "application/sparql-results+json")
                .body(LEADER_QUERY.to_string())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    return InvariantResult::Error {
                        reason: format!("SPARQL request failed: {}", e),
                    };
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return InvariantResult::Error {
                    reason: format!("SPARQL query returned {}: {}", status, body),
                };
            }

            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    return InvariantResult::Error {
                        reason: format!("reading response body: {}", e),
                    };
                }
            };

            let json: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    return InvariantResult::Error {
                        reason: format!("invalid JSON: {}", e),
                    };
                }
            };

            let bindings = json
                .pointer("/results/bindings")
                .and_then(|v| v.as_array());

            match bindings {
                Some(arr) if !arr.is_empty() => InvariantResult::Holds,
                Some(_) => InvariantResult::Violated {
                    reason: "no node holds picloud:hasRole picloud:Leader".to_string(),
                },
                None => InvariantResult::Error {
                    reason: "unexpected SPARQL response format".to_string(),
                },
            }
        }
    }
}
