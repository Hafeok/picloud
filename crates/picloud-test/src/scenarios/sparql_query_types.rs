//! ADR-005: SPARQL query types — execute SELECT, ASK, CONSTRUCT, DESCRIBE
//! queries and verify correct result formats.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SparqlQueryTypes;

#[async_trait]
impl Scenario for SparqlQueryTypes {
    fn name(&self) -> &str {
        "sparql-query-types"
    }

    fn adr(&self) -> &str {
        "ADR-005"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let mut issues: Vec<String> = Vec::new();

        // SELECT query — should return JSON with results/bindings.
        let select_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?s WHERE { ?s a picloud:Node } LIMIT 10
        "#;
        match assertions::sparql_query(ctx, select_query).await {
            Ok(body) => {
                let json: Result<serde_json::Value, _> = serde_json::from_str(&body);
                match json {
                    Ok(v) => {
                        if v.pointer("/results/bindings").is_none() {
                            issues.push(
                                "SELECT response missing /results/bindings".to_string(),
                            );
                        }
                    }
                    Err(e) => {
                        issues.push(format!("SELECT response is not valid JSON: {}", e));
                    }
                }
            }
            Err(e) => issues.push(format!("SELECT query failed: {}", e)),
        }

        // ASK query — should return JSON with boolean field.
        let ask_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK { ?s a picloud:Node }
        "#;
        match assertions::sparql_query(ctx, ask_query).await {
            Ok(body) => {
                let json: Result<serde_json::Value, _> = serde_json::from_str(&body);
                match json {
                    Ok(v) => {
                        if v.get("boolean").is_none() {
                            issues.push(
                                "ASK response missing 'boolean' field".to_string(),
                            );
                        }
                    }
                    Err(e) => {
                        issues.push(format!("ASK response is not valid JSON: {}", e));
                    }
                }
            }
            Err(e) => issues.push(format!("ASK query failed: {}", e)),
        }

        // CONSTRUCT query — should return non-empty triples with subject/predicate/object.
        let construct_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            CONSTRUCT { ?s a picloud:Node } WHERE { ?s a picloud:Node } LIMIT 10
        "#;
        match assertions::sparql_query(ctx, construct_query).await {
            Ok(body) => {
                if body.trim().is_empty() {
                    issues.push("CONSTRUCT returned empty body".to_string());
                } else if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                    // Verify CONSTRUCT returns triples (subject/predicate/object bindings)
                    let bindings = v.pointer("/results/bindings")
                        .or_else(|| v.get("results"))
                        .and_then(|r| r.as_array());
                    if let Some(arr) = bindings {
                        if arr.is_empty() {
                            issues.push("CONSTRUCT returned zero triples".to_string());
                        } else {
                            let first = &arr[0];
                            if first.get("subject").is_none() || first.get("predicate").is_none() || first.get("object").is_none() {
                                issues.push(format!(
                                    "CONSTRUCT triple missing subject/predicate/object fields: {}",
                                    serde_json::to_string(first).unwrap_or_default()
                                ));
                            }
                        }
                    }
                }
            }
            Err(e) => issues.push(format!("CONSTRUCT query failed: {}", e)),
        }

        // DESCRIBE query — should return non-empty triples.
        let describe_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            DESCRIBE ?s WHERE { ?s a picloud:Node } LIMIT 1
        "#;
        match assertions::sparql_query(ctx, describe_query).await {
            Ok(body) => {
                if body.trim().is_empty() {
                    issues.push("DESCRIBE returned empty body".to_string());
                } else if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                    let bindings = v.pointer("/results/bindings")
                        .or_else(|| v.get("results"))
                        .and_then(|r| r.as_array());
                    if let Some(arr) = bindings {
                        if arr.is_empty() {
                            issues.push("DESCRIBE returned zero triples".to_string());
                        }
                    }
                }
            }
            Err(e) => issues.push(format!("DESCRIBE query failed: {}", e)),
        }

        // Turtle content negotiation — request Turtle for a CONSTRUCT query
        {
            let url = format!("{}/graph", ctx.config.base_url());
            let construct_for_turtle = "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o } LIMIT 5";
            match ctx.http_client
                .get(&url)
                .query(&[("query", construct_for_turtle)])
                .header("Accept", "text/turtle")
                .send()
                .await
            {
                Ok(resp) => {
                    let ct = resp.headers().get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if !ct.contains("text/turtle") {
                        issues.push(format!("Turtle request got Content-Type: {ct} (expected text/turtle)"));
                    }
                    let body = resp.text().await.unwrap_or_default();
                    if body.trim().is_empty() {
                        issues.push("Turtle CONSTRUCT returned empty body".to_string());
                    }
                }
                Err(e) => issues.push(format!("Turtle CONSTRUCT request failed: {e}")),
            }
        }

        if issues.is_empty() {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "{} SPARQL query type issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
