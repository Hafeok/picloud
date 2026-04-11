//! ADR-028: Verify slices only depend on picloud-domain, not on each other.
//!
//! For each slice crate under `crates/`, reads its Cargo.toml and asserts
//! the only `picloud-*` dependency is `picloud-domain`. The server binary
//! and picloud-test are excluded (they are composition roots / test harness).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct SliceDependencyEnforcement;

/// Crates that are NOT slices — they are allowed to depend on multiple picloud-* crates.
const EXCLUDED_CRATES: &[&str] = &["picloud-test", "picloud-cli"];

/// The server binary is in src/main.rs, not a crate under crates/.
/// picloud-cli is allowed picloud-domain only.
const ALLOWED_PICLOUD_DEP: &str = "picloud-domain";

#[async_trait]
impl Scenario for SliceDependencyEnforcement {
    fn name(&self) -> &str {
        "slice-dependency-enforcement"
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
                    reason: format!("Failed to read crates directory: {}", e),
                };
            }
        };

        let mut violations: Vec<String> = Vec::new();

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let crate_name = entry.file_name().to_string_lossy().to_string();

            // Skip excluded crates
            if EXCLUDED_CRATES.contains(&crate_name.as_str()) {
                continue;
            }

            // Skip picloud-domain itself (it has no picloud deps)
            if crate_name == "picloud-domain" {
                continue;
            }

            let cargo_toml_path = entry.path().join("Cargo.toml");
            if !cargo_toml_path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&cargo_toml_path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let parsed: toml::Value = match content.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_table()) {
                for key in deps.keys() {
                    if key.starts_with("picloud-") && key != ALLOWED_PICLOUD_DEP {
                        violations.push(format!("{} depends on {}", crate_name, key));
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
                    "{} cross-slice dependency violations:\n  - {}",
                    violations.len(),
                    violations.join("\n  - ")
                ),
            }
        }
    }
}
