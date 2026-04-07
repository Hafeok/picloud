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

pub mod implementation;

pub use implementation::OxigraphProjector;
