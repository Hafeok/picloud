//! picloud-iam
//!
//! Implements the IdentityProvider and SecretStore traits from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;
pub mod secrets;

pub use implementation::{LocalIdentityProvider, StoredAppRegistration, StoredIdentity};
pub use secrets::InMemorySecretStore;
