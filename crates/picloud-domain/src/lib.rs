/// PiCloud Domain Model
///
/// This crate is the stable foundation of the entire platform.
/// Every other crate depends on this one. Nothing else depends on anything else.
///
/// Rules:
/// - No I/O, no network calls, no heavy runtime dependencies
/// - Only types, traits, errors, and the IRI model
/// - Every platform concept has a typed representation here
/// - Compiles fast — LLMs read this first to understand the platform

pub mod error;
pub mod iri;
pub mod resources;
pub mod events;
pub mod identity;
pub mod storage;
pub mod workload;
pub mod product;
pub mod traits;
pub mod parser;
