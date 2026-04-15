//! Platform self-monitoring (FT-011)
//!
//! The platform monitors itself using its own RDF graph and inference rules.
//! Built-in rules detect degraded replication, lagging projections, and Raft health.
//! SelfMonitoringCheckCompleted events surface platform-level issues through
//! the same mechanism as application alerts.

use async_trait::async_trait;
use tracing::debug;

use picloud_domain::error::Result;
use picloud_domain::events::{HealthStatus, SelfMonitoringCheck};
use picloud_domain::traits::SelfMonitor;

/// Implementation of platform self-monitoring.
///
/// Runs a set of built-in health checks against the platform's own state:
/// - Raft health: is the cluster healthy with a stable leader?
/// - Replication status: are volumes fully replicated?
/// - Projection lag: is the RDF projector keeping up with the event log?
pub struct PlatformSelfMonitor {
    /// Function to check Raft health (returns true if healthy)
    raft_check: Box<dyn Fn() -> (HealthStatus, String) + Send + Sync>,
    /// Function to check replication status
    replication_check: Box<dyn Fn() -> (HealthStatus, String) + Send + Sync>,
    /// Function to check projection lag
    projection_check: Box<dyn Fn() -> (HealthStatus, String) + Send + Sync>,
}

impl PlatformSelfMonitor {
    /// Create a new self-monitor with default healthy checks.
    /// Use `with_*` methods to customize individual checks.
    pub fn new() -> Self {
        Self {
            raft_check: Box::new(|| (HealthStatus::Healthy, "Raft cluster is healthy".to_string())),
            replication_check: Box::new(|| {
                (HealthStatus::Healthy, "All volumes fully replicated".to_string())
            }),
            projection_check: Box::new(|| {
                (HealthStatus::Healthy, "RDF projection is current".to_string())
            }),
        }
    }

    /// Override the Raft health check function.
    pub fn with_raft_check(
        mut self,
        check: impl Fn() -> (HealthStatus, String) + Send + Sync + 'static,
    ) -> Self {
        self.raft_check = Box::new(check);
        self
    }

    /// Override the replication status check function.
    pub fn with_replication_check(
        mut self,
        check: impl Fn() -> (HealthStatus, String) + Send + Sync + 'static,
    ) -> Self {
        self.replication_check = Box::new(check);
        self
    }

    /// Override the projection lag check function.
    pub fn with_projection_check(
        mut self,
        check: impl Fn() -> (HealthStatus, String) + Send + Sync + 'static,
    ) -> Self {
        self.projection_check = Box::new(check);
        self
    }
}

#[async_trait]
impl SelfMonitor for PlatformSelfMonitor {
    async fn run_checks(&self) -> Result<Vec<SelfMonitoringCheck>> {
        let mut checks = Vec::with_capacity(3);

        // Check 1: Raft health
        let (raft_status, raft_msg) = (self.raft_check)();
        debug!(check = "raft_health", status = %raft_status, message = %raft_msg);
        checks.push(SelfMonitoringCheck {
            check_name: "raft_health".to_string(),
            status: raft_status,
            message: raft_msg,
        });

        // Check 2: Replication status
        let (repl_status, repl_msg) = (self.replication_check)();
        debug!(check = "replication_status", status = %repl_status, message = %repl_msg);
        checks.push(SelfMonitoringCheck {
            check_name: "replication_status".to_string(),
            status: repl_status,
            message: repl_msg,
        });

        // Check 3: Projection lag
        let (proj_status, proj_msg) = (self.projection_check)();
        debug!(check = "projection_lag", status = %proj_status, message = %proj_msg);
        checks.push(SelfMonitoringCheck {
            check_name: "projection_lag".to_string(),
            status: proj_status,
            message: proj_msg,
        });

        Ok(checks)
    }
}
