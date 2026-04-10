//! ADR-054: Verify every scenario file named in ADR Test coverage sections exists.
//!
//! This scenario parses the ADR file, extracts all `.rs` scenario file names
//! referenced in `Test coverage` sections (lines like `- \`scenario_name.rs\``),
//! and checks that a corresponding file exists under
//! `crates/picloud-test/src/scenarios/`.

use std::collections::BTreeSet;
use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{ScenarioResult, Scenario, TestContext};

pub struct ScenarioCatalogueSync;

#[async_trait]
impl Scenario for ScenarioCatalogueSync {
    fn name(&self) -> &str {
        "scenario_catalogue_sync"
    }

    fn adr(&self) -> &str {
        "ADR-054"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        let adr_path = ctx.workspace_root().join("docs/picloud-adrs.md");
        let scenarios_dir = ctx.workspace_root().join("crates/picloud-test/src/scenarios");

        let content = match std::fs::read_to_string(&adr_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!(
                        "Failed to read ADR file at {}: {}",
                        adr_path.display(),
                        e
                    ),
                };
            }
        };

        // Extract scenario file names from Test coverage sections.
        let referenced = extract_scenario_filenames(&content);

        if referenced.is_empty() {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "No scenario file references found in ADR file".to_string(),
            };
        }

        let mut missing: Vec<String> = Vec::new();
        for filename in &referenced {
            let file_path = scenarios_dir.join(filename);
            if !file_path.exists() {
                missing.push(filename.clone());
            }
        }

        let duration = start.elapsed();

        if missing.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{}/{} ADR-referenced scenario files missing:\n  - {}",
                    missing.len(),
                    referenced.len(),
                    missing.join("\n  - ")
                ),
            }
        }
    }
}

/// Extract unique `.rs` filenames from lines like `- \`scenario_name.rs\`` in
/// Test coverage sections of the ADR file.
fn extract_scenario_filenames(content: &str) -> BTreeSet<String> {
    let mut filenames = BTreeSet::new();
    let mut in_test_coverage = false;

    for line in content.lines() {
        // Detect start of Test coverage section.
        if line.contains("**Test coverage:**") || line.contains("Test coverage:") {
            in_test_coverage = true;
            continue;
        }

        // Detect end of Test coverage section (next heading or next bold section).
        if in_test_coverage
            && (line.starts_with("## ") || line.starts_with("---"))
        {
            in_test_coverage = false;
            continue;
        }

        if !in_test_coverage {
            continue;
        }

        // Look for scenario file references: `- \`name.rs\` — description`
        // Also match lines under "Scenario tests:" subsection.
        if let Some(start) = line.find('`') {
            if let Some(end) = line[start + 1..].find('`') {
                let name = &line[start + 1..start + 1 + end];
                if name.ends_with(".rs") && !name.contains('/') && !name.contains(' ') {
                    filenames.insert(name.to_string());
                }
            }
        }
    }

    filenames
}
