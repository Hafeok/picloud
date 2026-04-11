//! ADR-058: Verify picloud-test compiles independently.
//!
//! This scenario runs `cargo check -p picloud-test` and asserts that
//! the command exits with status 0, confirming the test crate can be
//! built without pulling in the full workspace dependency graph.

use std::time::Instant;

use async_trait::async_trait;
use tokio::process::Command;

use crate::harness::runner::{ScenarioResult, Scenario, TestContext};

pub struct TestCrateBuildsIndependently;

#[async_trait]
impl Scenario for TestCrateBuildsIndependently {
    fn name(&self) -> &str {
        "test_crate_builds_independently"
    }

    fn adr(&self) -> &str {
        "ADR-058"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Use the workspace root so cargo resolves the workspace correctly.
        let workspace_root = ctx.workspace_root();

        let output = match Command::new(crate::harness::cargo::cargo_bin())
            .args(["check", "-p", "picloud-test"])
            .current_dir(&workspace_root)
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to spawn cargo check: {}", e),
                };
            }
        };

        let duration = start.elapsed();

        if output.status.success() {
            ScenarioResult::Pass { duration }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "cargo check -p picloud-test exited with {}: {}",
                    output.status,
                    stderr.chars().take(500).collect::<String>()
                ),
            }
        }
    }
}
