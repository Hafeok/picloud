//! ADR-010: OCI Containers and Raw Binaries — verify picloud-server supports workload scheduling.
//!
//! Checks the picloud-server binary help output for workload-related subcommands or
//! verifies source for workload module registration.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct BinaryWorkloadScenario;

#[async_trait]
impl Scenario for BinaryWorkloadScenario {
    fn name(&self) -> &str {
        "binary-workload"
    }

    fn adr(&self) -> &str {
        "ADR-010"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let server_bin = ctx.workspace_root().join("target/release/picloud-server");

        if server_bin.exists() {
            // Check help output for workload-related content.
            let output = match tokio::process::Command::new(&server_bin)
                .arg("--help")
                .output()
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!("failed to execute picloud-server --help: {}", e),
                    };
                }
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("{}{}", stdout, stderr);

            // Look for workload-related keywords in help.
            let workload_keywords = ["workload", "schedule", "container", "binary"];
            let found: Vec<&&str> = workload_keywords
                .iter()
                .filter(|kw| combined.to_lowercase().contains(&kw.to_lowercase()))
                .collect();

            if !found.is_empty() {
                info!(
                    keywords = ?found,
                    "picloud-server help references workload capabilities"
                );
                return ScenarioResult::Pass {
                    duration: start.elapsed(),
                };
            }

            info!("picloud-server help does not mention workloads; checking source");
        }

        // Fallback: check picloud-workload source and composition root.
        let workload_src = ctx.workspace_root().join("crates/picloud-workload/src");
        if !workload_src.exists() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-workload crate source directory does not exist".to_string(),
            };
        }

        let lib_rs = workload_src.join("lib.rs");
        let scheduler_rs = workload_src.join("scheduler.rs");

        let mut found_workload_code = false;

        for path in [&lib_rs, &scheduler_rs] {
            if let Ok(content) = std::fs::read_to_string(path) {
                let lower = content.to_lowercase();
                if lower.contains("workloadscheduler")
                    || lower.contains("schedule")
                    || lower.contains("binaryworkload")
                    || lower.contains("rawbinary")
                {
                    info!(file = %path.display(), "found workload scheduling code");
                    found_workload_code = true;
                    break;
                }
            }
        }

        // Check composition root for workload registration.
        let main_rs = ctx.workspace_root().join("src/main.rs");
        if let Ok(content) = std::fs::read_to_string(&main_rs) {
            if content.contains("picloud_workload") || content.contains("WorkloadScheduler") {
                info!("picloud-workload registered in composition root (src/main.rs)");
                found_workload_code = true;
            }
        }

        if found_workload_code {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no workload scheduling code found in picloud-workload or composition root".to_string(),
            }
        }
    }
}
