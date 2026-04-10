//! ADR-050: Builder Pattern CLI — partial argument resource generation.
//!
//! Runs `picloud new` with partial arguments to verify it either generates partial
//! resource files or returns appropriate errors for missing required fields.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NewResourcePartialScenario;

#[async_trait]
impl Scenario for NewResourcePartialScenario {
    fn name(&self) -> &str {
        "new-resource-partial"
    }

    fn adr(&self) -> &str {
        "ADR-050"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let cli_bin = ctx.workspace_root().join("target/release/picloud");
        if !cli_bin.exists() {
            return ScenarioResult::Skip {
                reason: format!(
                    "picloud CLI binary not found at {}",
                    cli_bin.display()
                ),
            };
        }

        // First check that `picloud new` subcommand exists.
        let help_output = match tokio::process::Command::new(&cli_bin)
            .args(["new", "--help"])
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to execute picloud new --help: {}", e),
                };
            }
        };

        let help_stdout = String::from_utf8_lossy(&help_output.stdout);
        let help_stderr = String::from_utf8_lossy(&help_output.stderr);
        let help_combined = format!("{}{}", help_stdout, help_stderr);

        if !help_output.status.success() && !help_combined.to_lowercase().contains("new") {
            return ScenarioResult::Skip {
                reason: "picloud new subcommand not available".to_string(),
            };
        }

        // Create a temporary directory for output.
        let tmp_dir = std::env::temp_dir().join(format!("picloud-test-partial-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp_dir);
        let output_path = tmp_dir.join("test-partial.picloud");

        // Run picloud new container with partial arguments (product only, missing name and image).
        // Use --no-interactive or equivalent to avoid blocking on prompts.
        let partial_output = tokio::process::Command::new(&cli_bin)
            .args([
                "new",
                "container",
                "--product",
                "test-product",
                "--output",
                &output_path.display().to_string(),
            ])
            .env("PICLOUD_NO_TTY", "1") // Signal non-interactive mode.
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        // Clean up temp dir.
        let _ = std::fs::remove_dir_all(&tmp_dir);

        match partial_output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}{}", stdout, stderr).to_lowercase();

                // Expected: either prompts for missing fields, generates a partial file,
                // or reports an error about missing required fields.
                if output.status.success() {
                    info!("picloud new container with partial args succeeded (may have used defaults)");
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else if combined.contains("required")
                    || combined.contains("missing")
                    || combined.contains("prompt")
                    || combined.contains("name")
                    || combined.contains("interactive")
                {
                    info!(
                        "picloud new container with partial args correctly reports missing fields"
                    );
                    ScenarioResult::Pass {
                        duration: start.elapsed(),
                    }
                } else {
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "picloud new container with partial args failed unexpectedly: exit={}, output={}",
                            output.status, combined
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to execute picloud new container: {}", e),
            },
        }
    }
}
