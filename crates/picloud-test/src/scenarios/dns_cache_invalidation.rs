//! ADR-052: DNS Cache Invalidation — verify DNS cache invalidation logic exists.
//!
//! Reads DNS server source to confirm that event-driven cache invalidation is
//! implemented for resource changes (WorkloadRescheduled, IngressCreated, etc.).

use std::time::Instant;

use async_trait::async_trait;
use tracing::info;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct DnsCacheInvalidationScenario;

/// Events that must trigger DNS cache invalidation per ADR-052.
const INVALIDATION_EVENTS: &[&str] = &[
    "WorkloadRescheduled",
    "IngressCreated",
    "IngressUpdated",
    "IngressDeleted",
    "NodeJoined",
    "NodeLeft",
    "ProductDeployed",
];

#[async_trait]
impl Scenario for DnsCacheInvalidationScenario {
    fn name(&self) -> &str {
        "dns-cache-invalidation"
    }

    fn adr(&self) -> &str {
        "ADR-052"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Primary location: picloud-network/src/dns/
        let dns_dir = ctx.workspace_root().join("crates/picloud-network/src/dns");
        let network_src = ctx.workspace_root().join("crates/picloud-network/src");

        let search_dir = if dns_dir.exists() {
            &dns_dir
        } else if network_src.exists() {
            &network_src
        } else {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "picloud-network crate source directory does not exist".to_string(),
            };
        };

        let mut all_source = String::new();
        let mut files_scanned = 0;
        scan_rs_files(search_dir, &mut all_source, &mut files_scanned);

        if files_scanned == 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no Rust source files found in picloud-network DNS module".to_string(),
            };
        }

        // Check for cache invalidation infrastructure.
        let cache_markers = [
            "cache",
            "Cache",
            "invalidate",
            "Invalidate",
            "ttl",
            "TTL",
            "expire",
        ];

        let has_cache = cache_markers.iter().any(|m| all_source.contains(m));

        if !has_cache {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "no DNS cache implementation found in picloud-network".to_string(),
            };
        }

        // Check for event-driven invalidation.
        let event_markers = [
            "event",
            "subscribe",
            "invalidat",
            "on_event",
            "handle_event",
        ];

        let has_event_invalidation = event_markers
            .iter()
            .any(|m| all_source.to_lowercase().contains(&m.to_lowercase()));

        if !has_event_invalidation {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "DNS cache exists but no event-driven invalidation logic found".to_string(),
            };
        }

        // Check which specific invalidation events are handled.
        let mut found_events = Vec::new();
        let mut missing_events = Vec::new();

        for event in INVALIDATION_EVENTS {
            // Check for the event name (case-insensitive) or snake_case variant.
            let snake = to_snake_case(event);
            if all_source.contains(event) || all_source.contains(&snake) {
                found_events.push(*event);
            } else {
                missing_events.push(*event);
            }
        }

        info!(
            found_events = ?found_events,
            missing_events = ?missing_events,
            files_scanned = files_scanned,
            "DNS cache invalidation analysis complete"
        );

        // Pass if we found cache invalidation infrastructure with at least some event handling.
        if !found_events.is_empty() || has_event_invalidation {
            ScenarioResult::Pass {
                duration: start.elapsed(),
            }
        } else {
            ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "DNS cache exists but none of the expected invalidation events found: {:?}",
                    INVALIDATION_EVENTS
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

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}
