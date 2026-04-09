//! picloud-storage
//!
//! Implements the StorageBackend trait from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.
//!
//! Phase 1 ships with `LocalStorageBackend`, which uses local filesystem
//! directories to simulate NVMe block storage.

pub mod implementation;
pub mod replication;

pub use implementation::LocalStorageBackend;
pub use implementation::LocalSnapshotManager;
pub use replication::StorageReplicator;
