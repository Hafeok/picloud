//! picloud-http
//!
//! HTTP server, IRI routing, and content negotiation for PiCloud.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod implementation;
pub mod inference;
pub mod metrics;
pub mod otel;
pub mod provisioner;
pub mod telemetry_store;

pub use implementation::{ContentType, IngressRoute, IngressTable, PiCloudHttpServer, new_ingress_table, resource_response};
pub use inference::InferenceEngine;
pub use metrics::MetricsAgent;
pub use otel::{OtelAggregator, OtelStream, parse_otlp_json};
pub use provisioner::Provisioner;
pub use telemetry_store::JsonlTelemetryStore;
