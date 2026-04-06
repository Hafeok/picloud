//! picloud-network
//!
//! Implements the domain traits from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;

pub use implementation::{
    InMemoryDnsRegistry, NodeCertificate, PlatformCa, ServiceCertificate,
};
