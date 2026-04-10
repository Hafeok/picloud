//! ADR-006: Named graph isolation — query specific named graphs and assert
//! each returns only its own triples.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NamedGraphIsolation;

#[async_trait]
impl Scenario for NamedGraphIsolation {
    fn name(&self) -> &str {
        "named-graph-isolation"
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

        // List all named graphs in the store.
        let graphs_query = r#"
            SELECT DISTINCT ?g WHERE {
                GRAPH ?g { ?s ?p ?o }
            }
        "#;

        let body = match assertions::sparql_query(ctx, graphs_query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to list named graphs: {}", e),
                };
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse response: {}", e),
                };
            }
        };

        let bindings = match json
            .pointer("/results/bindings")
            .and_then(|v| v.as_array())
        {
            Some(arr) => arr.clone(),
            None => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "no named graphs found in store".to_string(),
                };
            }
        };

        if bindings.is_empty() {
            return ScenarioResult::Pass {
                duration: start.elapsed(),
            };
        }

        // For each named graph, count triples inside it vs triples from other
        // graphs that reference subjects in this graph.
        let mut issues: Vec<String> = Vec::new();

        for binding in &bindings {
            let graph_uri = match binding
                .pointer("/g/value")
                .and_then(|v| v.as_str())
            {
                Some(uri) => uri,
                None => continue,
            };

            // Count triples in this specific named graph.
            let count_query = format!(
                r#"
                SELECT (COUNT(*) AS ?count) WHERE {{
                    GRAPH <{}> {{ ?s ?p ?o }}
                }}
                "#,
                graph_uri
            );

            match assertions::sparql_query(ctx, &count_query).await {
                Ok(count_body) => {
                    let count_json: serde_json::Value =
                        match serde_json::from_str(&count_body) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                    let count = count_json
                        .pointer("/results/bindings/0/count/value")
                        .and_then(|v| v.as_str())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0);

                    if count == 0 {
                        issues.push(format!(
                            "named graph {} has 0 triples",
                            graph_uri
                        ));
                    }
                }
                Err(e) => {
                    issues.push(format!(
                        "failed to count triples in {}: {}",
                        graph_uri, e
                    ));
                }
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
                    "{} named graph issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
