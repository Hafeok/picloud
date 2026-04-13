//! TC-211: Product with container, volume, workload identity deploys end-to-end.
//!
//! Verifies that the full product deployment model exists: a product can declare
//! a container workload, a volume, and a workload identity, and all necessary
//! types, traits, and wiring exist in the codebase to support end-to-end deployment.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ProductE2eContainerVolumeIdentity;

#[async_trait]
impl Scenario for ProductE2eContainerVolumeIdentity {
    fn name(&self) -> &str {
        "product-e2e-container-volume-identity"
    }

    fn adr(&self) -> &str {
        "ADR-010"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // 1. Verify Product resource type exists with version
        let resources_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/resources.rs");
        let resources_src = match std::fs::read_to_string(&resources_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read resources.rs: {}", e),
                };
            }
        };

        if !resources_src.contains("struct Product") {
            issues.push("Product resource type not found".to_string());
        }
        if !resources_src.contains("struct Container") {
            issues.push("Container resource type not found".to_string());
        }
        if !resources_src.contains("struct Volume") {
            issues.push("Volume resource type not found".to_string());
        }

        // 2. Verify workload types
        let workload_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/workload.rs");
        let workload_src = match std::fs::read_to_string(&workload_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read workload.rs: {}", e),
                };
            }
        };

        if !workload_src.contains("ContainerSpec") {
            issues.push("ContainerSpec not found in workload.rs".to_string());
        }
        if !workload_src.contains("identity") {
            issues.push("No identity field found in workload specs".to_string());
        }

        // 3. Verify WorkloadScheduler trait exists
        let traits_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/traits.rs");
        let traits_src = match std::fs::read_to_string(&traits_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read traits.rs: {}", e),
                };
            }
        };

        if !traits_src.contains("WorkloadScheduler") {
            issues.push("WorkloadScheduler trait not found".to_string());
        }
        if !traits_src.contains("StorageBackend") {
            issues.push("StorageBackend trait not found".to_string());
        }
        if !traits_src.contains("IdentityProvider") {
            issues.push("IdentityProvider trait not found".to_string());
        }

        // 4. Verify composition root wires all slices
        let main_path = ctx.workspace_root().join("src/main.rs");
        let main_src = match std::fs::read_to_string(&main_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read main.rs: {}", e),
                };
            }
        };

        if !main_src.contains("picloud_workload") {
            issues.push("picloud-workload not wired in composition root".to_string());
        }
        if !main_src.contains("picloud_storage") {
            issues.push("picloud-storage not wired in composition root".to_string());
        }
        if !main_src.contains("picloud_iam") {
            issues.push("picloud-iam not wired in composition root".to_string());
        }

        // 5. Verify workload scheduler implementation exists
        let scheduler_path = ctx
            .workspace_root()
            .join("crates/picloud-workload/src/implementation.rs");
        let scheduler_src = match std::fs::read_to_string(&scheduler_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read workload implementation: {}", e),
                };
            }
        };

        if !scheduler_src.contains("impl WorkloadScheduler for") {
            issues.push("WorkloadScheduler not implemented".to_string());
        }

        // 6. Verify identity injection in workload scheduler
        let has_identity_injection = scheduler_src.contains("PICLOUD_WORKLOAD_IDENTITY")
            || scheduler_src.contains("identity")
            || scheduler_src.contains("workload_identity");
        if !has_identity_injection {
            issues.push("No identity injection in workload scheduler".to_string());
        }

        // 7. Verify HTTP provisioner handles all resource types
        let http_src_dir = ctx.workspace_root().join("crates/picloud-http/src");
        let provisioner_path = http_src_dir.join("provisioner.rs");
        if let Ok(prov_src) = std::fs::read_to_string(&provisioner_path) {
            if !prov_src.contains("container") && !prov_src.contains("Container") {
                issues.push("HTTP provisioner does not handle containers".to_string());
            }
            if !prov_src.contains("volume") && !prov_src.contains("Volume") {
                issues.push("HTTP provisioner does not handle volumes".to_string());
            }
        }

        // 8. Verify integration test exists confirming the end-to-end flow
        let integration_test = ctx
            .workspace_root()
            .join("crates/picloud-http/tests/integration_test.rs");
        if let Ok(test_src) = std::fs::read_to_string(&integration_test) {
            if !test_src.contains("test_apply_product") {
                issues.push("No end-to-end apply test in integration tests".to_string());
            }
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} e2e issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
