//! ADR-054: Verify every Accepted ADR contains a Test coverage section.
//!
//! This scenario parses the ADR file and asserts that every ADR with
//! status `Accepted` contains a `Test coverage` section with at least
//! one scenario test reference and at least one exit criterion.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{ScenarioResult, Scenario, TestContext};

pub struct AdrTestCoverageCompleteness;

#[async_trait]
impl Scenario for AdrTestCoverageCompleteness {
    fn name(&self) -> &str {
        "adr_test_coverage_completeness"
    }

    fn adr(&self) -> &str {
        "ADR-054"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Locate the ADR file relative to the workspace root.
        let adr_path = ctx.workspace_root().join("docs/picloud-adrs.md");

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

        // Parse ADRs: split on `## ADR-NNN:` headings.
        let adrs = parse_adrs(&content);

        let mut missing: Vec<String> = Vec::new();

        for adr in &adrs {
            if !adr.is_accepted {
                continue;
            }

            let has_test_coverage = adr.has_test_coverage_section;
            let has_scenario = adr.has_scenario_test;
            let has_exit = adr.has_exit_criteria;

            if !has_test_coverage {
                missing.push(format!("{}: missing Test coverage section", adr.id));
            } else {
                if !has_scenario {
                    missing.push(format!(
                        "{}: Test coverage section has no scenario test references",
                        adr.id
                    ));
                }
                if !has_exit {
                    missing.push(format!(
                        "{}: Test coverage section has no exit criteria",
                        adr.id
                    ));
                }
            }
        }

        let duration = start.elapsed();

        if missing.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} ADR(s) with incomplete test coverage:\n  - {}",
                    missing.len(),
                    missing.join("\n  - ")
                ),
            }
        }
    }
}

struct ParsedAdr {
    id: String,
    is_accepted: bool,
    has_test_coverage_section: bool,
    has_scenario_test: bool,
    has_exit_criteria: bool,
}

/// Parse ADR file content into structured ADR records.
fn parse_adrs(content: &str) -> Vec<ParsedAdr> {
    let mut adrs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // Detect ADR heading: `## ADR-NNN: ...`
        if let Some(rest) = line.strip_prefix("## ADR-") {
            let id = if let Some(colon_pos) = rest.find(':') {
                format!("ADR-{}", &rest[..colon_pos])
            } else {
                format!("ADR-{}", rest.trim())
            };

            // Determine the end of this ADR section (next `## ADR-` or EOF).
            let section_start = i + 1;
            let mut section_end = lines.len();
            for j in section_start..lines.len() {
                if lines[j].starts_with("## ADR-") {
                    section_end = j;
                    break;
                }
            }

            let section_lines = &lines[section_start..section_end];
            let section_text: String = section_lines.join("\n");

            let is_accepted = section_lines.iter().any(|l| {
                l.contains("**Status:**") && l.contains("Accepted")
            });

            let has_test_coverage_section = section_lines.iter().any(|l| {
                l.contains("**Test coverage:**") || l.contains("Test coverage:")
            });

            // Look for scenario test references (lines like `- \`name.rs\``)
            let has_scenario_test = section_lines.iter().any(|l| {
                l.contains(".rs`") && l.trim().starts_with('-')
            });

            let has_exit_criteria = section_text.contains("Exit criteria:")
                || section_text.contains("exit criteria:");

            adrs.push(ParsedAdr {
                id,
                is_accepted,
                has_test_coverage_section,
                has_scenario_test,
                has_exit_criteria,
            });

            i = section_end;
        } else {
            i += 1;
        }
    }

    adrs
}
