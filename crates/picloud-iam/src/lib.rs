//! picloud-iam
//!
//! Implements the IdentityProvider trait from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;

pub use implementation::{LocalIdentityProvider, StoredIdentity};
