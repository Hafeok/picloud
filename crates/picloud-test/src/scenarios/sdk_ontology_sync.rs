//! ADR-033: Verify SDK generation uses ontology definitions from the product.
//!
//! Reads picloud-sdk-gen source to confirm that the generator references
//! the cluster domain (and thus the ontology namespace) when generating
//! SDK code, ensuring SDKs stay in sync with the platform ontology.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SdkOntologySync;

#[async_trait]
impl Scenario for SdkOntologySync {
    fn name(&self) -> &str {
        "sdk-ontology-sync"
    }

    fn adr(&self) -> &str {
        "ADR-033"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // 1. Read the SDK generator implementation
        let impl_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/src/implementation.rs");
        let impl_content = match std::fs::read_to_string(&impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", impl_path.display(), e),
                };
            }
        };

        // 2. Verify SdkGenerator references ClusterDomain (ontology namespace root)
        if !impl_content.contains("ClusterDomain") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator does not reference ClusterDomain — SDK cannot sync with ontology namespace".to_string(),
            };
        }

        // 3. Verify the generator has a cluster_domain field
        if !impl_content.contains("cluster_domain") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator missing cluster_domain field for ontology binding".to_string(),
            };
        }

        // 4. Verify the lib.rs re-exports the generator types
        let lib_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/src/lib.rs");
        let lib_content = match std::fs::read_to_string(&lib_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", lib_path.display(), e),
                };
            }
        };

        if !lib_content.contains("SdkGenerator") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "SdkGenerator not re-exported from picloud-sdk-gen lib.rs".to_string(),
            };
        }

        // 5. Verify Cargo.toml depends on picloud-domain (ontology types)
        let cargo_path = ctx
            .workspace_root()
            .join("crates/picloud-sdk-gen/Cargo.toml");
        let cargo_content = match std::fs::read_to_string(&cargo_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to read {}: {}", cargo_path.display(), e),
                };
            }
        };

        if !cargo_content.contains("picloud-domain") {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-sdk-gen does not depend on picloud-domain for ontology types"
                    .to_string(),
            };
        }

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
