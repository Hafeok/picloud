//! picloud-rdf
//!
//! Implements the `StateProjector` trait from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.
//!
//! Uses Oxigraph as an embedded RDF triplestore (in-memory or disk-backed).
//! Events are projected into RDF triples; state is queried via SPARQL.
//!
//! Distribution model: the graph is NOT replicated directly. Each node
//! projects its own local copy from the Raft-replicated event log.
//! On restart, `OxigraphProjector::open()` restores from disk and replays
//! only the events missed since the last shutdown.

pub mod data_product;
pub mod implementation;
pub mod projection_runner;
pub mod rdf_store;

pub use data_product::OxigraphDataProductProjector;
pub use implementation::OxigraphProjector;
pub use projection_runner::{
    DataProductProjectionRunner, DataProductRegistration, ProjectionOutcome,
};
pub use rdf_store::OxigraphRdfStoreManager;
