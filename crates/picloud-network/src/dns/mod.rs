/// Authoritative DNS Server — ADR-052
///
/// Serves DNS queries for the cluster's tenant domain.
/// Answers come from the RDF graph via Oxigraph SPARQL queries.
/// Cache is in-memory with 30s TTL, invalidated by platform events.
///
/// Authoritative for: picloud.local (or custom tenant domain)
/// Record types: A, SRV, TXT, PTR
/// No recursion. No upstream forwarding. NXDOMAIN for unknowns.
///
/// Integration: Pi-hole conditional forwarding -> any cluster node:53

pub mod server;
pub mod resolver;
pub mod cache;
pub mod records;
pub mod events;
