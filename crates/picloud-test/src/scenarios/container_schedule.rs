//! ADR-010: Read workload scheduler source. Verify container scheduling types
//! exist and check for container runtime integration (youki, podman, docker).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ContainerSchedule;

#[async_trait]
impl Scenario for ContainerSchedule {
    fn name(&self) -> &str {
        "container_schedule"
    }

    fn adr(&self) -> &str {
        "ADR-010"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // Read the workload scheduler implementation.
        let scheduler_path = ctx
            .workspace_root()
            .join("crates/picloud-workload/src/implementation.rs");

        let scheduler_src = match std::fs::read_to_string(&scheduler_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read workload scheduler: {}", e),
                };
            }
        };

        // Verify the WorkloadScheduler trait is implemented.
        if !scheduler_src.contains("impl WorkloadScheduler for")
            && !scheduler_src.contains("WorkloadScheduler")
        {
            issues.push(
                "WorkloadScheduler trait not implemented in picloud-workload".to_string(),
            );
        }

        // Verify container scheduling types exist.
        let workload_types_path = ctx
            .workspace_root()
            .join("crates/picloud-domain/src/workload.rs");

        let workload_src = match std::fs::read_to_string(&workload_types_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read workload types: {}", e),
                };
            }
        };

        if !workload_src.contains("ContainerSpec") {
            issues.push("ContainerSpec type not found in workload.rs".to_string());
        }

        if !workload_src.contains("BinarySpec") {
            issues.push("BinarySpec type not found in workload.rs".to_string());
        }

        // Check for container runtime integration.
        // ADR-010 specifies youki as the OCI runtime, but the implementation
        // may also support podman/docker as alternatives.
        let has_youki = scheduler_src.contains("youki");
        let has_podman = scheduler_src.contains("podman") || scheduler_src.contains("Podman");
        let has_docker = scheduler_src.contains("docker") || scheduler_src.contains("Docker");

        if !has_youki && !has_podman && !has_docker {
            issues.push(
                "No container runtime integration found (expected youki, podman, or docker)"
                    .to_string(),
            );
        }

        // Verify the scheduler can handle both container and binary workloads.
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
            issues.push("WorkloadScheduler trait not defined in traits.rs".to_string());
        }

        // Verify the scheduler tracks workload status.
        if !scheduler_src.contains("WorkloadStatus")
            && !scheduler_src.contains("Running")
        {
            issues.push(
                "Scheduler does not track workload status".to_string(),
            );
        }

        // Verify the scheduler assigns PIDs or container IDs.
        let tracks_process = scheduler_src.contains("pid")
            || scheduler_src.contains("container_id")
            || scheduler_src.contains("Child");
        if !tracks_process {
            issues.push(
                "Scheduler does not track process IDs or container IDs".to_string(),
            );
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} scheduler issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
