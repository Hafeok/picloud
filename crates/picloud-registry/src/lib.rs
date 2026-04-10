//! picloud-registry
//!
//! Embedded OCI Distribution v1.1-compliant registry (ADR-054).
//! Stores container images on the platform's block storage pool.
//! Content-addressed blobs are deduplicated by digest.
//!
//! Depends only on picloud-domain — never on other slices.

pub mod blob_store;
pub mod gc;
pub mod implementation;

pub use blob_store::BlobStore;
pub use gc::GarbageCollector;
pub use implementation::LocalRegistryBackend;
