//! ADR-028: Verify no picloud slice crate imports another slice crate.
//!
//! Scans every `Cargo.toml` under `crates/picloud-*` and asserts that
//! no slice depends on another slice — only `picloud-domain` is allowed.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct NoCrossSliceImports;

#[async_trait]
impl Scenario for NoCrossSliceImports {
    fn name(&self) -> &str {
        "no_cross_slice_imports"
    }

    fn adr(&self) -> &str {
        "ADR-028"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let crates_dir = ctx.workspace_root().join("crates");

        let entries = match std::fs::read_dir(&crates_dir) {
            Ok(e) => e,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("cannot read crates dir: {}", e),
                };
            }
        };

        let slice_crates: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("picloud-")
                    && name != "picloud-domain"
                    && name != "picloud-test"
                    && e.path().join("Cargo.toml").exists()
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        let mut violations = Vec::new();

        for crate_name in &slice_crates {
            let cargo_path = crates_dir.join(crate_name).join("Cargo.toml");
            let content = match std::fs::read_to_string(&cargo_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Check [dependencies] for other picloud-* crates (excluding picloud-domain)
            let mut in_deps = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("[dependencies]") || trimmed.starts_with("[dependencies.") {
                    in_deps = true;
                    continue;
                }
                if trimmed.starts_with('[') && !trimmed.starts_with("[dependencies") {
                    in_deps = false;
                    continue;
                }
                if in_deps && trimmed.starts_with("picloud-") {
                    let dep_name = trimmed.split('=').next().unwrap_or("").trim();
                    if dep_name != "picloud-domain" {
                        violations.push(format!(
                            "{} depends on {}",
                            crate_name, dep_name
                        ));
                    }
                }
            }
        }

        let duration = start.elapsed();

        if violations.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} cross-slice dependency violation(s):\n  - {}",
                    violations.len(),
                    violations.join("\n  - ")
                ),
            }
        }
    }
}
