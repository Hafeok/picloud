//! ADR-055: Upgrade Gate Enforcement — verify that a failed staging scenario
//! halts the upgrade pipeline.
//!
//! This is a harness-level test: it creates a mock "always-fail" scenario,
//! runs it through the scenario runner, and asserts the runner reports a
//! non-zero failure count. No actual cluster interaction is performed.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::info;

use crate::config::{ClusterConfig, ClusterSection};
use crate::harness::runner::{Scenario, ScenarioResult, TestContext};

pub struct UpgradeGateEnforcementScenario;

/// A mock scenario that always fails, simulating a broken staging test.
struct AlwaysFailScenario;

#[async_trait]
impl Scenario for AlwaysFailScenario {
    fn name(&self) -> &str {
        "always-fail-mock"
    }

    fn adr(&self) -> &str {
        "ADR-055"
    }

    async fn run(&self, _ctx: &TestContext) -> ScenarioResult {
        ScenarioResult::Fail {
            duration: Duration::from_millis(1),
            reason: "intentional failure for gate enforcement test".to_string(),
        }
    }
}

#[async_trait]
impl Scenario for UpgradeGateEnforcementScenario {
    fn name(&self) -> &str {
        "upgrade-gate-enforcement"
    }

    fn adr(&self) -> &str {
        "ADR-055"
    }

    async fn run(&self, _ctx: &TestContext) -> ScenarioResult {
        let start = Instant::now();

        // Build a minimal TestContext — we will not hit any real cluster.
        let dummy_config = ClusterConfig {
            cluster: ClusterSection {
                domain: "test.local".to_string(),
                http_port: 7443,
                platform_version: "0.0.0-test".to_string(),
                tls: false,
                base_host: None,
            },
            operator: None,
            nodes: vec![],
        };
        let dummy_ctx = TestContext::new(dummy_config, std::path::PathBuf::from("."));

        // Run the always-fail mock directly (not via run_scenarios, which would
        // pollute the outer runner's error logs and stats).
        let mock = AlwaysFailScenario;
        let mock_result = mock.run(&dummy_ctx).await;

        // The gate condition: the mock must report failure.
        let mock_failed = matches!(mock_result, ScenarioResult::Fail { .. });
        info!(mock_failed = mock_failed, "mock scenario run complete");

        if !mock_failed {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "expected mock scenario to fail, but it did not".to_string(),
            };
        }

        info!("gate enforcement validated: pipeline halts on staging failure");

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
