//! ADR-058: Verify picloud-test has no internal slice imports.
//!
//! This scenario reads `crates/picloud-test/Cargo.toml`, parses its
//! `[dependencies]` section, and asserts the only `picloud-*` dependency
//! is `picloud-domain`. Any other picloud crate dependency violates the
//! dependency rule from ADR-058.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{ScenarioResult, Scenario, TestContext};

pub struct NoInternalImports;

#[async_trait]
impl Scenario for NoInternalImports {
    fn name(&self) -> &str {
        "no_internal_imports"
    }

    fn adr(&self) -> &str {
        "ADR-058"
    }

    async fn run(&self, _ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let cargo_toml_path =
            std::path::Path::new(manifest_dir).join("Cargo.toml");

        let content = match std::fs::read_to_string(&cargo_toml_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "Failed to read {}: {}",
                        cargo_toml_path.display(),
                        e
                    ),
                };
            }
        };

        let parsed: toml::Value = match content.parse() {
            Ok(v) => v,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Failed to parse Cargo.toml: {}", e),
                };
            }
        };

        // Collect all picloud-* dependency names.
        let mut violations: Vec<String> = Vec::new();

        if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_table()) {
            for key in deps.keys() {
                if key.starts_with("picloud-") && key != "picloud-domain" {
                    violations.push(key.clone());
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
                    "picloud-test has {} forbidden picloud-* dependencies: {}",
                    violations.len(),
                    violations.join(", ")
                ),
            }
        }
    }
}
