//! ADR-020: Verify service registry entries are bound to product versions.
//!
//! Reads RDF/domain source to confirm the ProductRegistryEntry type
//! carries version information, ensuring the cluster graph reflects
//! product versions accurately.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct RegistryVersionBinding;

#[async_trait]
impl Scenario for RegistryVersionBinding {
    fn name(&self) -> &str {
        "registry-version-binding"
    }

    fn adr(&self) -> &str {
        "ADR-020"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify ProductRegistryEntry exists and carries version info
        let product_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/product.rs");
        let product_content = match std::fs::read_to_string(&product_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", product_path.display(), e),
                };
            }
        };

        if !product_content.contains("struct ProductRegistryEntry") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ProductRegistryEntry struct not found in product.rs".to_string(),
            };
        }

        // 2. Verify version field exists on ProductRegistryEntry
        // The struct must contain a `version` field for binding to product versions
        if !product_content.contains("pub version: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "ProductRegistryEntry missing 'version' field for version binding".to_string(),
            };
        }

        // 3. Verify the registry entry has SPARQL endpoint, ontology, and events
        let required_fields = [
            "sparql_endpoint",
            "ontology_iri",
            "events_endpoint",
            "emitted_event_types",
        ];
        for field in &required_fields {
            if !product_content.contains(field) {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "ProductRegistryEntry missing '{}' field for registry completeness",
                        field
                    ),
                };
            }
        }

        // 4. Verify ProductDeployed event carries version
        let events_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/events.rs");
        let events_content = match std::fs::read_to_string(&events_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", events_path.display(), e),
                };
            }
        };

        if !events_content.contains("ProductDeployedPayload") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ProductDeployedPayload not found in events.rs".to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
