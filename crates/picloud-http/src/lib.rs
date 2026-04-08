//! picloud-http
//!
//! HTTP server, IRI routing, and content negotiation for PiCloud.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;
pub mod metrics;
pub mod provisioner;

pub use implementation::{ContentType, IngressRoute, IngressTable, PiCloudHttpServer, new_ingress_table, resource_response};
pub use metrics::MetricsAgent;
pub use provisioner::Provisioner;
