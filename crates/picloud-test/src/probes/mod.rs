use std::future::Future;

use crate::harness::runner::TestContext;

/// Result of a protocol probe.
pub enum ProbeResult {
    /// The probe succeeded.
    Ok,
    /// The probe detected an issue.
    Failed { reason: String },
    /// The probe could not be executed (e.g. dependency not available).
    Error { reason: String },
}

/// A protocol-level probe that verifies a specific network service is reachable
/// and responding correctly (DNS, HTTP, OCI registry, mTLS handshake, etc.).
pub trait ProtocolProbe: Send + Sync {
    fn name(&self) -> &str;
    fn protocol(&self) -> &str; // "DNS", "HTTP", "OCI", etc.
    fn run(&self, ctx: &TestContext) -> impl Future<Output = ProbeResult> + Send;
}
