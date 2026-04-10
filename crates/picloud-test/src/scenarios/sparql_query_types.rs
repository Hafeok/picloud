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

        // CONSTRUCT query — should return non-empty RDF.
        let construct_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            CONSTRUCT { ?s a picloud:Node } WHERE { ?s a picloud:Node } LIMIT 10
        "#;
        match assertions::sparql_query(ctx, construct_query).await {
            Ok(body) => {
                if body.trim().is_empty() {
                    issues.push("CONSTRUCT returned empty body".to_string());
                }
            }
            Err(e) => issues.push(format!("CONSTRUCT query failed: {}", e)),
        }

        // DESCRIBE query — should return non-empty RDF.
        let describe_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            DESCRIBE ?s WHERE { ?s a picloud:Node } LIMIT 1
        "#;
        match assertions::sparql_query(ctx, describe_query).await {
            Ok(body) => {
                if body.trim().is_empty() {
                    issues.push("DESCRIBE returned empty body".to_string());
                }
            }
            Err(e) => issues.push(format!("DESCRIBE query failed: {}", e)),
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
