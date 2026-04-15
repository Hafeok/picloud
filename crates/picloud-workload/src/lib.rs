//! picloud-workload
//!
//! Implements the domain traits from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;

pub use implementation::ProcessScheduler;
pub use implementation::ContainerRuntime;
pub use implementation::InMemoryDrainCoordinator;

// Re-export W3C trace context utilities (FT-048)
pub use implementation::ProcessScheduler as TraceparentGenerator;
