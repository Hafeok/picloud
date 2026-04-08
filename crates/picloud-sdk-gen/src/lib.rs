//! picloud-sdk-gen
//!
//! Template-based SDK generation for Rust, TypeScript, and .NET clients.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;

pub use implementation::{SdkGenerator, SdkGenerationResult, SdkLanguage, SdkPublishResult};
