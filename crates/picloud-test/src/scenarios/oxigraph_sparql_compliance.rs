//! ADR-006: Oxigraph SPARQL 1.1 compliance — run SELECT with FILTER, ASK,
//! CONSTRUCT, and DESCRIBE queries and assert valid responses.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OxigraphSparqlCompliance;

#[async_trait]
impl Scenario for OxigraphSparqlCompliance {
    fn name(&self) -> &str {
        "oxigraph-sparql-compliance"
    }

    fn adr(&self) -> &str {
        "ADR-006"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        let mut issues: Vec<String> = Vec::new();

        // SELECT with FILTER.
        let select_filter = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?s WHERE {
                ?s a picloud:Node .
                FILTER(BOUND(?s))
            } LIMIT 5
        "#;
        match assertions::sparql_query(ctx, select_filter).await {
            Ok(body) => {
                if serde_json::from_str::<serde_json::Value>(&body).is_err() {
                    issues.push("SELECT+FILTER response is not valid JSON".to_string());
                }
            }
            Err(e) => issues.push(format!("SELECT+FILTER failed: {}", e)),
        }

        // ASK.
        let ask = r#"
            ASK { ?s ?p ?o }
        "#;
        match assertions::sparql_query(ctx, ask).await {
            Ok(body) => {
                let json: Result<serde_json::Value, _> = serde_json::from_str(&body);
                match json {
                    Ok(v) => {
                        if v.get("boolean").is_none() {
                            issues.push("ASK response missing 'boolean' field".to_string());
                        }
                    }
                    Err(e) => {
                        issues.push(format!("ASK response not valid JSON: {}", e));
                    }
                }
            }
            Err(e) => issues.push(format!("ASK query failed: {}", e)),
        }

        // CONSTRUCT.
        let construct = r#"
            CONSTRUCT { ?s ?p ?o }
            WHERE { ?s ?p ?o }
            LIMIT 5
        "#;
        match assertions::sparql_query(ctx, construct).await {
            Ok(body) => {
                if body.trim().is_empty() {
                    issues.push("CONSTRUCT returned empty body".to_string());
                }
            }
            Err(e) => issues.push(format!("CONSTRUCT query failed: {}", e)),
        }

        // DESCRIBE.
        let describe = r#"
            DESCRIBE ?s WHERE { ?s ?p ?o } LIMIT 1
        "#;
        match assertions::sparql_query(ctx, describe).await {
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
                    "{} compliance issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
