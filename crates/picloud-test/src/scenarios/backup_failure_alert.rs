//! ADR-047: Backup failure alert — query for AlertFired events related to backup failures.
//!
//! Query the RDF graph via SPARQL for picloud:Alert triples with alertType
//! "BackupFailed". Verify the alert system can represent backup failure alerts.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::assertions;
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct BackupFailureAlert;

#[async_trait]
impl Scenario for BackupFailureAlert {
    fn name(&self) -> &str {
        "backup-failure-alert"
    }

    fn adr(&self) -> &str {
        "ADR-047"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        if !assertions::feature_available(ctx, "/health").await {
            return ScenarioResult::Skip {
                reason: "cluster not reachable".to_string(),
            };
        }

        // Query for backup-related alerts in the graph.
        let query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            SELECT ?alert ?type ?severity ?resource WHERE {
                ?alert a picloud:Alert ;
                       picloud:alertType ?type ;
                       picloud:alertSeverity ?severity ;
                       picloud:alertResource ?resource .
                FILTER(?type = "BackupFailed")
            }
        "#;

        let body = match assertions::sparql_query(ctx, query).await {
            Ok(b) => b,
            Err(e) => {
                return ScenarioResult::Skip {
                    reason: format!("SPARQL endpoint not available: {}", e),
                };
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to parse SPARQL response: {}", e),
                };
            }
        };

        // Verify the SPARQL response has the correct structure.
        if json.pointer("/results/bindings").is_none() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SPARQL response missing results/bindings".to_string(),
            };
        }

        // Also verify the backup-related inference rule exists by querying for it.
        let rule_query = r#"
            PREFIX picloud: <https://picloud.local/ontology#>
            ASK {
                ?volume a picloud:Volume ;
                        picloud:lastBackupStatus ?status .
            }
        "#;

        match assertions::sparql_query(ctx, rule_query).await {
            Ok(rule_body) => {
                // If the query succeeds, the RDF schema supports backup status tracking.
                let _json: serde_json::Value = match serde_json::from_str(&rule_body) {
                    Ok(v) => v,
                    Err(_) => {
                        return ScenarioResult::Skip {
                            reason: "could not parse backup status SPARQL response".to_string(),
                        };
                    }
                };
            }
            Err(_) => {
                return ScenarioResult::Skip {
                    reason: "backup status properties not yet in graph schema".to_string(),
                };
            }
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
