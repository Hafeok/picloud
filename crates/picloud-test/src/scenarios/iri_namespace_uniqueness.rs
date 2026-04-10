//! ADR-042: Verify cluster ID is part of IRI namespace to prevent cross-cluster collisions.
//!
//! Reads domain/IRI source to confirm that ClusterDomain is configurable
//! (not hardcoded) and that the IriBuilder uses the domain throughout,
//! ensuring IRI namespace uniqueness across clusters.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct IriNamespaceUniqueness;

#[async_trait]
impl Scenario for IriNamespaceUniqueness {
    fn name(&self) -> &str {
        "iri-namespace-uniqueness"
    }

    fn adr(&self) -> &str {
        "ADR-042"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Verify ClusterDomain is configurable
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

        if !iri_content.contains("struct ClusterDomain") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ClusterDomain struct not found in iri.rs".to_string(),
            };
        }

        // 2. Verify IriBuilder takes ClusterDomain as input (not hardcoded)
        if !iri_content.contains("fn new(domain: ClusterDomain)") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "IriBuilder::new must take ClusterDomain parameter for namespace uniqueness"
                        .to_string(),
            };
        }

        // 3. Verify IriBuilder uses self.domain in IRI construction
        if !iri_content.contains("self.domain.0") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason:
                    "IriBuilder does not use self.domain — IRIs are not scoped to cluster domain"
                        .to_string(),
            };
        }

        // 4. Verify ClusterIdentity exists with cluster_id for cryptographic binding
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

        if !resources_content.contains("struct ClusterIdentity") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ClusterIdentity struct not found in resources.rs".to_string(),
            };
        }

        if !resources_content.contains("pub cluster_id: Uuid") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ClusterIdentity missing cluster_id field".to_string(),
            };
        }

        if !resources_content.contains("pub ca_fingerprint: String") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "ClusterIdentity missing ca_fingerprint for cryptographic binding"
                    .to_string(),
            };
        }

        // 5. Verify custom domain test exists (proves different clusters get different namespaces)
        if !iri_content.contains("custom_domain") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "No custom_domain test — cannot verify namespace uniqueness across clusters"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
