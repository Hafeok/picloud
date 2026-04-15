//! picloud-events
//!
//! Implements the EventLog trait from picloud-domain::traits.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;
pub mod router;

pub use implementation::{InMemoryEventLog, PersistentEventLog, ProductEventStore, WriteCallback, WriteThroughEventLog};
pub use router::PlatformEventRouter;
