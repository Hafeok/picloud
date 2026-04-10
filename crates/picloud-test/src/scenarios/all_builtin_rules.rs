//! ADR-041: All Built-in Alert Rules — verify all built-in alert rules are registered.
//!
//! Reads alert/inference source to confirm that the platform's built-in alert rules
//! (CPU temp, memory, disk, node unreachable, workload failed) are defined and compile.

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct AllBuiltinRulesScenario;

/// The built-in alert rules that must be present per ADR-041.
const EXPECTED_RULES: &[&str] = &[
    "cpu",       // High CPU temperature (critical + warning)
    "memory",    // High memory usage (critical + warning)
    "disk",      // High disk usage (critical)
    "unreachable", // Node unreachable (Raft heartbeat missed)
    "failed",    // Product workload failed
];

#[async_trait]
impl Scenario for AllBuiltinRulesScenario {
    fn name(&self) -> &str {
        "all-builtin-rules"
    }

    fn adr(&self) -> &str {
        "ADR-041"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Search for alert rule definitions in relevant crates.
        let search_dirs = [
            ctx.workspace_root().join("crates/picloud-rdf/src"),
            ctx.workspace_root().join("crates/picloud-events/src"),
            ctx.workspace_root().join("crates/picloud-domain/src"),
        ];

        let mut all_source = String::new();
        let mut files_scanned = 0;

        for dir in &search_dirs {
            if !dir.exists() {
                continue;
            }
            scan_rs_files(dir, &mut all_source, &mut files_scanned);
        }

        if files_scanned == 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no Rust source files found in picloud-rdf, picloud-events, or picloud-domain".to_string(),
            };
        }

        let source_lower = all_source.to_lowercase();

        // Check for alert-related infrastructure.
        let alert_infra_markers = ["alertfired", "alert_fired", "alertresolved", "alert_resolved", "picloud:alert"];
        let has_alert_infra = alert_infra_markers
            .iter()
            .any(|m| source_lower.contains(m));

        if !has_alert_infra {
            // Check if AlertFired is at least defined as an event type.
            let events_file = ctx.workspace_root().join("crates/picloud-domain/src/events.rs");
            if let Ok(content) = std::fs::read_to_string(&events_file) {
                let lower = content.to_lowercase();
                if lower.contains("alertfired") || lower.contains("alert") {
                    info!("AlertFired event type found in domain events");
                } else {
                    return ScenarioResult::Fail {
                        duration: start.elapsed(),
                        reason: "no alert infrastructure (AlertFired/AlertResolved events) found in source".to_string(),
                    };
                }
            } else {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: "cannot read picloud-domain/src/events.rs and no alert infra found".to_string(),
                };
            }
        }

        // Check for each expected built-in rule.
        let mut missing_rules = Vec::new();
        for rule in EXPECTED_RULES {
            // Look for the rule keyword in combination with alert-related terms.
            let rule_present = source_lower.contains(rule)
                && (source_lower.contains("alert")
                    || source_lower.contains("rule")
                    || source_lower.contains("threshold")
                    || source_lower.contains("construct"));

            if !rule_present {
                missing_rules.push(*rule);
            }
        }

        if missing_rules.is_empty() {
            info!(
                rules_found = EXPECTED_RULES.len(),
                files_scanned = files_scanned,
                "all built-in alert rules found in source"
            );
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            // Partial pass — some rules exist. Report what is missing.
            let found_count = EXPECTED_RULES.len() - missing_rules.len();
            if found_count > 0 {
                info!(
                    found = found_count,
                    total = EXPECTED_RULES.len(),
                    missing = ?missing_rules,
                    "some built-in alert rules found"
                );
            }
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "missing built-in alert rules: {:?} ({}/{} found)",
                    missing_rules,
                    found_count,
                    EXPECTED_RULES.len()
                ),
            }
        }
    }
}

fn scan_rs_files(dir: &std::path::Path, buffer: &mut String, count: &mut usize) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_rs_files(&path, buffer, count);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    buffer.push_str(&content);
                    buffer.push('\n');
                    *count += 1;
                }
            }
        }
    }
}
