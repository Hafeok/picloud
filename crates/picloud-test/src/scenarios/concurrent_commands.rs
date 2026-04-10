//! ADR-008: Verify that concurrent command handling is supported by checking
//! for async handlers and Arc-based shared state in the event handling code.

use std::time::Instant;

use async_trait::async_trait;

use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct ConcurrentCommands;

#[async_trait]
impl Scenario for ConcurrentCommands {
    fn name(&self) -> &str {
        "concurrent_commands"
    }

    fn adr(&self) -> &str {
        "ADR-008"
    }

    async fn run(&self, ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();
        let mut issues: Vec<String> = Vec::new();

        // Read the event log implementation.
        let events_impl_path = ctx
            .workspace_root()
            .join("crates/picloud-events/src/implementation.rs");

        let events_src = match std::fs::read_to_string(&events_impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read event log implementation: {}", e),
                };
            }
        };

        // Verify the event log uses async trait implementation.
        if !events_src.contains("#[async_trait]") && !events_src.contains("async fn append") {
            issues.push(
                "Event log does not use async handlers — concurrent commands may block"
                    .to_string(),
            );
        }

        // Verify Arc-based state sharing for thread safety.
        if !events_src.contains("Arc<") {
            issues.push(
                "Event log does not use Arc for shared state — not safe for concurrent access"
                    .to_string(),
            );
        }

        // Verify RwLock or Mutex for concurrent access protection.
        let has_rwlock = events_src.contains("RwLock");
        let has_mutex = events_src.contains("Mutex");
        if !has_rwlock && !has_mutex {
            issues.push(
                "Event log has no RwLock or Mutex — concurrent writes are not safe".to_string(),
            );
        }

        // Read the HTTP implementation to verify async handlers.
        let http_impl_path = ctx
            .workspace_root()
            .join("crates/picloud-http/src/implementation.rs");

        let http_src = match std::fs::read_to_string(&http_impl_path) {
            Ok(c) => c,
            Err(e) => {
                return ScenarioResult::Fail {
                    duration: start.elapsed(),
                    reason: format!("Cannot read HTTP implementation: {}", e),
                };
            }
        };

        // Verify HTTP handlers are async.
        if !http_src.contains("async fn handle_command")
            && !http_src.contains("async fn handle_apply")
        {
            issues.push(
                "HTTP command handlers are not async — cannot handle concurrent commands"
                    .to_string(),
            );
        }

        // Verify the HTTP server uses Arc for shared state injection.
        if !http_src.contains("Arc<") {
            issues.push(
                "HTTP server does not use Arc for shared state — not safe for concurrency"
                    .to_string(),
            );
        }

        // Verify the broadcast channel exists for pub/sub (concurrent subscribers).
        if !events_src.contains("broadcast") {
            issues.push(
                "Event log does not use broadcast channel for concurrent subscribers".to_string(),
            );
        }

        let duration = start.elapsed();

        if issues.is_empty() {
            ScenarioResult::Pass { duration }
        } else {
            ScenarioResult::Fail {
                duration,
                reason: format!(
                    "{} concurrency issue(s): {}",
                    issues.len(),
                    issues.join("; ")
                ),
            }
        }
    }
}
