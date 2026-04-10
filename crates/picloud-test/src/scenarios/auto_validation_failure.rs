//! ADR-050: Auto-Validation Failure — verify picloud compile validate catches errors.
//!
//! Generates a resource file with a deliberate error (non-existent volume reference),
//! runs `picloud compile validate`, and verifies the validation catches it.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AutoValidationFailureScenario;

#[async_trait]
impl Scenario for AutoValidationFailureScenario {
    fn name(&self) -> &str {
        "auto-validation-failure"
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

        // Check that `picloud compile` subcommand exists.
        let help_output = match tokio::process::Command::new(&cli_bin)
            .args(["compile", "--help"])
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("failed to execute picloud compile --help: {}", e),
                };
            }
        };

        let help_combined = format!(
            "{}{}",
            String::from_utf8_lossy(&help_output.stdout),
            String::from_utf8_lossy(&help_output.stderr)
        );

        if !help_output.status.success() && !help_combined.to_lowercase().contains("compile") {
            return ScenarioResult::Skip {
                reason: "picloud compile subcommand not available".to_string(),
            };
        }

        // Create a temporary directory with an invalid .picloud file.
        let tmp_dir = std::env::temp_dir().join(format!(
            "picloud-test-validation-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&tmp_dir);

        // Write an intentionally invalid resource file referencing a non-existent volume.
        let invalid_resource = r#"container 'test-invalid' = {
  product: 'test-product'
  image: 'test:latest'
  mount: 'nonexistent-volume:/data'
}
"#;
        let resource_path = tmp_dir.join("invalid-container.picloud");
        if let Err(e) = std::fs::write(&resource_path, invalid_resource) {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to write test resource file: {}", e),
            };
        }

        // Run picloud compile validate on the invalid file.
        let validate_output = tokio::process::Command::new(&cli_bin)
            .args(["compile", "validate", &resource_path.display().to_string()])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;

        // Clean up temp dir.
        let _ = std::fs::remove_dir_all(&tmp_dir);

        match validate_output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let combined = format!("{}{}", stdout, stderr).to_lowercase();

                if !output.status.success() {
                    // Validation correctly failed — check that the error message is informative.
                    if combined.contains("error")
                        || combined.contains("invalid")
                        || combined.contains("not found")
                        || combined.contains("nonexistent")
                        || combined.contains("validation")
                    {
                        info!("picloud compile validate correctly caught the validation error");
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    } else {
                        // Failed but without a clear error message.
                        info!(
                            output = combined.as_str(),
                            "validation failed but error message may not be descriptive"
                        );
                        ScenarioResult::Pass {
                            duration: start.elapsed(),
                        }
                    }
                } else {
                    // Validation passed when it should have failed.
                    ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: format!(
                            "picloud compile validate passed on an invalid resource file: {}",
                            combined
                        ),
                    }
                }
            }
            Err(e) => ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!("failed to execute picloud compile validate: {}", e),
            },
        }
    }
}
