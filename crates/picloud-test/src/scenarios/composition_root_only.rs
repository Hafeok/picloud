//! ADR-034: Composition root only — verify src/main.rs imports all slice
//! crates, and each slice only imports picloud-domain.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct CompositionRootOnly;

/// Slice crates expected to be wired in the composition root.
const SLICE_CRATES: &[&str] = &[
    "picloud-cluster",
    "picloud-events",
    "picloud-rdf",
    "picloud-iam",
    "picloud-storage",
    "picloud-workload",
    "picloud-network",
    "picloud-http",
];

/// Crates excluded from the slice-only-depends-on-domain check.
const EXCLUDED_FROM_CHECK: &[&str] = &[
    "picloud-domain",
    "picloud-test",
    "picloud-cli",
    "picloud-sdk-gen",
];

#[async_trait]
impl Scenario for CompositionRootOnly {
    fn name(&self) -> &str {
        "composition-root-only"
    }

    fn adr(&self) -> &str {
        "ADR-034"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // 1. Read src/main.rs and verify it imports slice crates.
        let main_path = ctx.workspace_root().join("src/main.rs");
        let main_src = match std::fs::read_to_string(&main_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("cannot read src/main.rs: {}", e),
                };
            }
        };

        for slice in SLICE_CRATES {
            let use_name = slice.replace('-', "_");
            if !main_src.contains(&use_name) {
                issues.push(format!(
                    "src/main.rs does not reference {}",
                    slice
                ));
            }
        }

        // 2. Read each slice's Cargo.toml and verify it only imports picloud-domain.
        let crates_dir = ctx.workspace_root().join("crates");
        let entries = match std::fs::read_dir(&crates_dir) {
            Ok(e) => e,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("cannot read crates directory: {}", e),
                };
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let crate_name = entry.file_name().to_string_lossy().to_string();

            if EXCLUDED_FROM_CHECK.contains(&crate_name.as_str()) {
                continue;
            }

            let cargo_toml = entry.path().join("Cargo.toml");
            if !cargo_toml.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&cargo_toml) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let parsed: toml::Value = match content.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_table()) {
                for key in deps.keys() {
                    if key.starts_with("picloud-") && key != "picloud-domain" {
                        issues.push(format!(
                            "{} depends on {} (should only depend on picloud-domain)",
                            crate_name, key
                        ));
                    }
                }
            }
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} composition root issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
