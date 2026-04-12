//! Test coverage for HTTP endpoints added in ADR gap closure:
//! - POST /products/:name/upgrade (ADR-017)
//! - POST/GET/DELETE /products/:name/subscriptions (ADR-018)
//! - GET /products/:name/ontology/:version (ADR-023)
//! - Accept: text/turtle content negotiation (ADR-008)

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct HttpEndpointCoverage;

#[async_trait]
impl Scenario for HttpEndpointCoverage {
    fn name(&self) -> &str {
        "http-endpoint-coverage"
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

        let base = ctx.config.base_url();
        let mut issues: Vec<String> = Vec::new();

        // --- ADR-017: Product upgrade endpoint ---
        {
            let resp = ctx.http_client
                .post(format!("{base}/products/endpoint-test/upgrade"))
                .json(&serde_json::json!({"version": "2.0.0"}))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if !r.status().is_success() {
                        issues.push(format!("upgrade endpoint returned {}", r.status()));
                    } else {
                        let body: serde_json::Value = r.json().await.unwrap_or_default();
                        if body["status"] != "upgrade_started" {
                            issues.push(format!("upgrade response unexpected: {body}"));
                        }
                        info!("upgrade endpoint OK");
                    }
                }
                Err(e) => issues.push(format!("upgrade endpoint failed: {e}")),
            }
        }

        // --- ADR-018: Subscription CRUD ---
        {
            // CREATE
            let resp = ctx.http_client
                .post(format!("{base}/products/endpoint-test/subscriptions"))
                .json(&serde_json::json!({
                    "name": "test-sub",
                    "source_product": "source-app",
                    "event_type": "TestEvent",
                    "handler": "handle_test"
                }))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if r.status().as_u16() != 201 {
                        issues.push(format!("subscription create returned {}", r.status()));
                    } else {
                        info!("subscription create OK");
                    }
                }
                Err(e) => issues.push(format!("subscription create failed: {e}")),
            }

            // LIST
            let resp = ctx.http_client
                .get(format!("{base}/products/endpoint-test/subscriptions"))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if !r.status().is_success() {
                        issues.push(format!("subscription list returned {}", r.status()));
                    } else {
                        info!("subscription list OK");
                    }
                }
                Err(e) => issues.push(format!("subscription list failed: {e}")),
            }

            // DELETE
            let resp = ctx.http_client
                .delete(format!("{base}/products/endpoint-test/subscriptions/test-sub"))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if !r.status().is_success() {
                        issues.push(format!("subscription delete returned {}", r.status()));
                    } else {
                        info!("subscription delete OK");
                    }
                }
                Err(e) => issues.push(format!("subscription delete failed: {e}")),
            }

            // Validation: missing fields should return 400
            let resp = ctx.http_client
                .post(format!("{base}/products/endpoint-test/subscriptions"))
                .json(&serde_json::json!({"name": "bad"}))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if r.status().as_u16() != 400 {
                        issues.push(format!(
                            "subscription create with missing fields returned {} (expected 400)",
                            r.status()
                        ));
                    } else {
                        info!("subscription validation OK");
                    }
                }
                Err(e) => issues.push(format!("subscription validation request failed: {e}")),
            }
        }

        // --- ADR-023: Versioned ontology endpoint ---
        {
            let resp = ctx.http_client
                .get(format!("{base}/products/endpoint-test/ontology/1.0.0"))
                .header("Accept", "text/turtle")
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if !r.status().is_success() {
                        issues.push(format!("versioned ontology returned {}", r.status()));
                    } else {
                        let ct = r.headers().get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");
                        if !ct.contains("text/turtle") {
                            issues.push(format!("versioned ontology Content-Type: {ct}"));
                        }
                        let body = r.text().await.unwrap_or_default();
                        if !body.contains("owl:Ontology") {
                            issues.push("versioned ontology body missing owl:Ontology".to_string());
                        }
                        if !body.contains("1.0.0") {
                            issues.push("versioned ontology body missing version".to_string());
                        }
                        info!("versioned ontology endpoint OK");
                    }
                }
                Err(e) => issues.push(format!("versioned ontology failed: {e}")),
            }
        }

        // --- ADR-008: Turtle content negotiation on product endpoint ---
        {
            let resp = ctx.http_client
                .get(format!("{base}/products/endpoint-test"))
                .header("Accept", "text/turtle")
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if !r.status().is_success() {
                        issues.push(format!("turtle product returned {}", r.status()));
                    } else {
                        let ct = r.headers().get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");
                        if !ct.contains("turtle") && !ct.contains("json") {
                            issues.push(format!("turtle product Content-Type unexpected: {ct}"));
                        }
                        info!("turtle content negotiation OK");
                    }
                }
                Err(e) => issues.push(format!("turtle content negotiation failed: {e}")),
            }
        }

        let duration = start.elapsed();
        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!("{} issue(s): {}", issues.len(), issues.join("; ")),
            }
        }
    }
}
