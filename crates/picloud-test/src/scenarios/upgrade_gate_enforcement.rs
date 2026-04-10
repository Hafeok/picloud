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
use crate::harness::runner::{run_scenarios, Scenario, ScenarioResult, TestContext};

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
            },
            operator: None,
            nodes: vec![],
        };
        let dummy_ctx = TestContext::new(dummy_config);

        // Create a scenario list containing only the always-fail mock.
        let scenarios: Vec<Box<dyn Scenario>> = vec![Box::new(AlwaysFailScenario)];

        // Run through the standard runner with no filters.
        let (passed, failed, skipped) =
            run_scenarios(&scenarios, &dummy_ctx, None, None).await;

        info!(
            passed = passed,
            failed = failed,
            skipped = skipped,
            "mock scenario run complete"
        );

        // The gate condition: if any scenario fails, the pipeline must report failure.
        if failed == 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: "expected failed > 0, but runner reported zero failures".to_string(),
            };
        }

        // Verify no scenarios passed (the only scenario should have failed).
        if passed > 0 {
            return ScenarioResult::Fail {
                duration: start.elapsed(),
                reason: format!(
                    "expected zero passes when all scenarios fail, got {}",
                    passed
                ),
            };
        }

        info!("gate enforcement validated: pipeline halts on staging failure");

        ScenarioResult::Pass {
            duration: start.elapsed(),
        }
    }
}
