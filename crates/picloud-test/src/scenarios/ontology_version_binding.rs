//! ADR-023: Verify ontology files are bound to product versions.
//!
//! Reads domain/RDF source to confirm that the Ontology resource type
//! exists and is bound to a product via ResourceMeta, and that the
//! IriBuilder can generate versioned ontology IRIs.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct OntologyVersionBinding;

#[async_trait]
impl Scenario for OntologyVersionBinding {
    fn name(&self) -> &str {
        "ontology-version-binding"
    }

    fn adr(&self) -> &str {
        "ADR-023"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify Ontology resource type exists
        let resources_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/resources.rs");
        let resources_content = match std::fs::read_to_string(&resources_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", resources_path.display(), e),
                };
            }
        };

        if !resources_content.contains("struct Ontology") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Ontology resource type not found in resources.rs".to_string(),
            };
        }

        // 2. Verify Ontology has file_path and format fields
        if !resources_content.contains("pub file_path: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Ontology missing file_path field".to_string(),
            };
        }

        if !resources_content.contains("pub format: OntologyFormat") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Ontology missing format field (OntologyFormat)".to_string(),
            };
        }

        // 3. Verify OntologyFormat supports Turtle and SHACL
        if !resources_content.contains("Turtle") || !resources_content.contains("Shacl") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "OntologyFormat must support both Turtle and Shacl".to_string(),
            };
        }

        // 4. Verify Ontology has a served_at IRI (dereferenceable)
        if !resources_content.contains("pub served_at: ResourceIri") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "Ontology missing served_at IRI for version binding".to_string(),
            };
        }

        // 5. Verify IriBuilder has product_ontology method
        let iri_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/iri.rs");
        let iri_content = match std::fs::read_to_string(&iri_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", iri_path.display(), e),
                };
            }
        };

        if !iri_content.contains("product_ontology") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "IriBuilder missing product_ontology method for versioned ontology IRIs"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
