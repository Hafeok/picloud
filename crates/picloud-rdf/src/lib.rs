//! picloud-rdf
//!
//! Implements the `StateProjector` trait from picloud-domain.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.
//!
//! Uses Oxigraph as an embedded in-memory RDF triplestore.
//! Events are projected into RDF triples; state is queried via SPARQL.

pub mod implementation;

pub use implementation::OxigraphProjector;
