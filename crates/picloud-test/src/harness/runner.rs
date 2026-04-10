use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tracing::{error, info, warn};

use crate::config::ClusterConfig;

/// Shared context passed to every scenario, invariant, and probe.
pub struct TestContext {
    pub config: ClusterConfig,
    pub http_client: reqwest::Client,
    workspace_root: PathBuf,
}

impl TestContext {
    pub fn new(config: ClusterConfig, workspace_root: PathBuf) -> Self {
        // Build an HTTP client that accepts self-signed certs (cluster uses internal CA).
        let http_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        Self {
            config,
            http_client,
            workspace_root,
        }
    }

    /// Path to the picloud workspace root directory.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
}

/// Result of running a single scenario.
pub enum ScenarioResult {
    Pass { duration: Duration },
    Fail { duration: Duration, reason: String },
    Skip { reason: String },
}

/// A named, self-contained integration test scenario.
#[async_trait]
pub trait Scenario: Send + Sync {
    /// Human-readable name for this scenario.
    fn name(&self) -> &str;

    /// The ADR this scenario validates, e.g. "ADR-055".
    fn adr(&self) -> &str;

    /// Execute the scenario against the cluster described by `ctx`.
    async fn run(&self, ctx: &TestContext) -> ScenarioResult;
}

/// Run a set of scenarios and return (passed, failed, skipped) counts.
pub async fn run_scenarios(
    scenarios: &[Box<dyn Scenario>],
    ctx: &TestContext,
    suite_filter: Option<&str>,
    scenario_filter: Option<&str>,
) -> (usize, usize, usize) {
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for scenario in scenarios {
        // Apply filters
        if let Some(filter) = scenario_filter {
            if scenario.name() != filter {
                continue;
            }
        }
        if let Some(filter) = suite_filter {
            if !scenario.adr().contains(filter) && !scenario.name().contains(filter) {
                continue;
            }
        }

        info!(
            scenario = scenario.name(),
            adr = scenario.adr(),
            "running scenario"
        );

        let start = Instant::now();
        let result = scenario.run(ctx).await;
        let elapsed = start.elapsed();

        match &result {
            ScenarioResult::Pass { duration } => {
                info!(
                    scenario = scenario.name(),
                    duration_ms = duration.as_millis() as u64,
                    "PASS"
                );
                passed += 1;
            }
            ScenarioResult::Fail { duration, reason } => {
                error!(
                    scenario = scenario.name(),
                    duration_ms = duration.as_millis() as u64,
                    reason = reason.as_str(),
                    "FAIL"
                );
                failed += 1;
            }
            ScenarioResult::Skip { reason } => {
                warn!(
                    scenario = scenario.name(),
                    reason = reason.as_str(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    "SKIP"
                );
                skipped += 1;
            }
        }
    }

    (passed, failed, skipped)
}
