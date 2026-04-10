//! ADR-010: Workload Identity Injection — verify identity injection mechanism exists.
//!
//! Reads workload source to confirm that workloads receive injected identity credentials
//! via environment variables (PICLOUD_WORKLOAD_IDENTITY and related).

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct WorkloadIdentityInjectionScenario;

#[async_trait]
impl Scenario for WorkloadIdentityInjectionScenario {
    fn name(&self) -> &str {
        "workload-identity-injection"
    }

    fn adr(&self) -> &str {
        "ADR-010"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Search for identity injection in picloud-workload source.
        let workload_src = ctx.workspace_root().join("crates/picloud-workload/src");
        if !workload_src.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-workload crate source directory does not exist".to_string(),
            };
        }

        let injection_markers = [
            "PICLOUD_WORKLOAD_IDENTITY",
            "workload_identity",
            "inject_identity",
            "identity_injection",
            "credential_inject",
            "env_inject",
        ];

        let mut found_injection = false;
        let mut found_in_file = String::new();
        let mut found_marker = String::new();

        // Scan all .rs files in picloud-workload/src.
        if let Ok(entries) = std::fs::read_dir(&workload_src) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "rs").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for marker in &injection_markers {
                            if content.contains(marker) {
                                found_injection = true;
                                found_in_file = path.display().to_string();
                                found_marker = marker.to_string();
                                break;
                            }
                        }
                        if found_injection {
                            break;
                        }
                    }
                }
            }
        }

        // Also check picloud-domain for WorkloadIdentity types.
        if !found_injection {
            let domain_files = [
                ctx.workspace_root().join("crates/picloud-domain/src/identity.rs"),
                ctx.workspace_root().join("crates/picloud-domain/src/workload.rs"),
                ctx.workspace_root().join("crates/picloud-domain/src/traits.rs"),
            ];

            for path in &domain_files {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if content.contains("WorkloadIdentity")
                        || content.contains("workload_identity")
                        || content.contains("IdentityInjection")
                    {
                        found_injection = true;
                        found_in_file = path.display().to_string();
                        found_marker = "WorkloadIdentity type".to_string();
                        break;
                    }
                }
            }
        }

        if found_injection {
            info!(
                file = found_in_file.as_str(),
                marker = found_marker.as_str(),
                "workload identity injection mechanism found"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no workload identity injection mechanism found in picloud-workload or picloud-domain".to_string(),
            }
        }
    }
}
