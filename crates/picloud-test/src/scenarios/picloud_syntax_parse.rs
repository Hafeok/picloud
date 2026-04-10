//! ADR-007: Find `.picloud` example files in the workspace and validate they
//! parse without errors. Skip if no `.picloud` files are found.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PicloudSyntaxParse;

#[async_trait]
impl Scenario for PicloudSyntaxParse {
    fn name(&self) -> &str {
        "picloud_syntax_parse"
    }

    fn adr(&self) -> &str {
        "ADR-007"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Recursively find all .picloud files in the workspace.
        let mut picloud_files: Vec<std::path::PathBuf> = Vec::new();
        find_picloud_files(ctx.workspace_root(), &mut picloud_files);

        if picloud_files.is_empty() {
            return ScenarioResult::Skip {
                reason: "No .picloud files found in the workspace".to_string(),
            };
        }

        let mut failures: Vec<String> = Vec::new();

        for file in &picloud_files {
            let content = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    let rel = file
                        .strip_prefix(ctx.workspace_root())
                        .unwrap_or(file)
                        .to_string_lossy();
                    failures.push(format!("{}: read error: {}", rel, e));
                    continue;
                }
            };

            // .picloud files use JSON format (ADR-007, Phase 1).
            // Attempt to parse as a ResourceFile JSON structure.
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(val) => {
                    // Verify it has a "resources" array at the top level.
                    if val.get("resources").and_then(|r| r.as_array()).is_none() {
                        let rel = file
                            .strip_prefix(ctx.workspace_root())
                            .unwrap_or(file)
                            .to_string_lossy();
                        failures.push(format!(
                            "{}: missing or non-array 'resources' field",
                            rel
                        ));
                    }
                }
                Err(e) => {
                    let rel = file
                        .strip_prefix(ctx.workspace_root())
                        .unwrap_or(file)
                        .to_string_lossy();
                    failures.push(format!("{}: parse error: {}", rel, e));
                }
            }
        }

        let duration = start.elapsed();

        if failures.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{}/{} .picloud files failed validation: {}",
                    failures.len(),
                    picloud_files.len(),
                    failures.join("; ")
                ),
            }
        }
    }
}

/// Recursively find all `.picloud` files under a directory, skipping `target/`.
fn find_picloud_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target directory and .git.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == ".git" {
                continue;
            }
            find_picloud_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("picloud") {
            out.push(path);
        }
    }
}
