/// Certificate Authority — ADR-053, ADR-055
///
/// The cluster CA lives in Raft state, encrypted at rest.
/// Every node that becomes leader can issue certificates immediately.
///
/// Two enrollment modes:
///   auto   — any node on the network gets a cert (home lab default)
///   token  — node must present a valid enrollment token
///
/// Modules:
///   authority   — CA key management, certificate signing, CSR validation
///   enrollment  — /enroll endpoint handler, CSR validation
///   renewal     — certificate expiry tracking, auto-renewal
///   revocation  — CRL management, Raft-replicated
///   smallstep   — Smallstep BYO-CA backend (ADR-030)
///   acme        — Native ACME server, HTTP-01 (ADR-055)

pub mod authority;
pub mod enrollment;
pub mod renewal;
pub mod revocation;
pub mod smallstep;
pub mod acme;
