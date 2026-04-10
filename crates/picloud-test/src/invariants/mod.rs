pub mod leader_always_present;

use std::future::Future;

use crate::harness::runner::TestContext;

/// Result of an invariant check.
pub enum InvariantResult {
    /// The invariant holds.
    Holds,
    /// The invariant is violated.
    Violated { reason: String },
    /// The check could not be performed.
    Error { reason: String },
}

/// A reusable invariant check that can be composed into scenarios.
///
/// Invariants represent conditions that must always be true in a healthy cluster,
/// e.g. "every node reports the same Raft leader", "all volumes have at least
/// one replica", etc.
pub trait Invariant: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, ctx: &TestContext) -> impl Future<Output = InvariantResult> + Send;
}
