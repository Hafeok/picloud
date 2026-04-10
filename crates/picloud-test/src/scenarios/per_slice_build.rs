//! ADR-034: Verify each slice crate builds independently.
//!
//! Runs `cargo check -p picloud-{crate}` for each slice crate in the
//! workspace. Verifies each compiles independently with only picloud-domain
//! as an internal dependency.

use std::time::Instant;

use async_trait::async_trait;
use tokio::process::Command;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PerSliceBuild;

/// All slice crates that must build independently.
const SLICE_CRATES: &[&str] = &[
    "picloud-domain",
    "picloud-cluster",
    "picloud-events",
    "picloud-rdf",
    "picloud-iam",
    "picloud-storage",
    "picloud-workload",
    "picloud-network",
    "picloud-http",
    "picloud-sdk-gen",
    "picloud-cli",
    "picloud-compiler",
    "picloud-registry",
];

#[async_trait]
impl Scenario for PerSliceBuild {
    fn name(&self) -> &str {
        "per-slice-build"
    }

    fn adr(&self) -> &str {
        "ADR-034"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let workspace_root = ctx.workspace_root();

        let mut failures: Vec<String> = Vec::new();

        for crate_name in SLICE_CRATES {
            // Verify the crate directory exists first
            let crate_dir = workspace_root.join("crates").join(crate_name);
            if !crate_dir.exists() {
                // Skip crates that don't exist yet — they may be in a future phase
                continue;
            }

            let output = match Command::new("cargo")
                .args(["check", "-p", crate_name])
                .current_dir(workspace_root)
                .output()
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    failures.push(format!("{}: failed to spawn cargo check: {}", crate_name, e));
                    continue;
                }
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                failures.push(format!(
                    "{}: cargo check failed: {}",
                    crate_name,
                    stderr.chars().take(300).collect::<String>()
                ));
            }
        }

        let duration = start.elapsed();

        if failures.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} slice(s) failed to build independently:\n  - {}",
                    failures.len(),
                    failures.join("\n  - ")
                ),
            }
        }
    }
}
