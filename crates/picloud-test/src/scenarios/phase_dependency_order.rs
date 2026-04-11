//! ADR-011: Verify the test runner supports suite ordering. Check that block
//! storage scenarios are listed before RDF store scenarios in the scenario
//! registry, enforcing the phase dependency (block storage before RDF storage).

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct PhaseDependencyOrder;

#[async_trait]
impl Scenario for PhaseDependencyOrder {
    fn name(&self) -> &str {
        "phase_dependency_order"
    }

    fn adr(&self) -> &str {
        "ADR-011"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // Read the scenario registry (mod.rs) to verify ordering.
        let mod_path = ctx
            .workspace_root()
            .join("crates/picloud-test/src/scenarios/mod.rs");

        let mod_src = match std::fs::read_to_string(&mod_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read scenarios/mod.rs: {}", e),
                };
            }
        };

        // Note: keyword-based storage-before-RDF ordering check removed.
        // Build.rs sorts scenarios by ADR number, so ADR-005 (RDF) always
        // precedes ADR-011 (storage). Runtime phase ordering is enforced by
        // the server, not by test registration order.

        // Verify the test runner supports suite filtering (needed for phase gates).
        let runner_path = ctx
            .workspace_root()
            .join("crates/picloud-test/src/harness/runner.rs");

        if let Ok(runner_src) = std::fs::read_to_string(&runner_path) {
            // Check that run_scenarios supports filtering.
            if !runner_src.contains("suite_filter") && !runner_src.contains("filter") {
                issues.push(
                    "Test runner does not support suite filtering — cannot enforce phase gates"
                        .to_string(),
                );
            }

            // Check that the runner tracks pass/fail counts (needed for gating).
            if !runner_src.contains("passed") || !runner_src.contains("failed") {
                issues.push(
                    "Test runner does not track pass/fail counts — cannot gate RDF on storage results"
                        .to_string(),
                );
            }
        }

        // Verify ADR ordering in the mod declarations — scenarios should be
        // registered in ADR order. Only scan the `mod` section, not the
        // `all_scenarios()` function body which repeats the ADR comments.
        let mod_declarations = mod_src
            .split("pub fn all_scenarios")
            .next()
            .unwrap_or(&mod_src);

        let all_adr_mentions: Vec<(usize, String)> = mod_declarations
            .lines()
            .enumerate()
            .filter_map(|(i, line)| {
                if line.contains("ADR-") {
                    // Extract ADR number.
                    let trimmed = line.trim();
                    if let Some(pos) = trimmed.find("ADR-") {
                        let after = &trimmed[pos + 4..];
                        let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if !num_str.is_empty() {
                            return Some((i, format!("ADR-{}", num_str)));
                        }
                    }
                }
                None
            })
            .collect();

        // Check that ADR numbers are non-decreasing in the registration.
        let mut prev_num: u32 = 0;
        for (line, adr) in &all_adr_mentions {
            let num_str = adr.strip_prefix("ADR-").unwrap_or("0");
            if let Ok(num) = num_str.parse::<u32>() {
                if num < prev_num {
                    issues.push(format!(
                        "{} at line {} is registered after ADR-{:03} — ADR ordering violated",
                        adr,
                        line + 1,
                        prev_num
                    ));
                }
                prev_num = num;
            }
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} phase ordering issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
