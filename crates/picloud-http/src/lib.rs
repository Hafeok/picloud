//! picloud-http
//!
//! HTTP server, IRI routing, and content negotiation for PiCloud.
//! Depends only on picloud-domain — never on other slices.
//! Slices communicate at runtime via the event log.

pub mod alerts;
pub mod capability;
pub mod data_product_monitor;
pub mod implementation;
pub mod inference;
pub mod ingress;
pub mod iri;
pub mod metrics;
pub mod otel;
pub mod parquet_store;
pub mod provisioner;
pub mod proxy;
pub mod router;
pub mod self_monitor;
pub mod telemetry_store;
pub mod tls;

pub use alerts::BuiltInAlertEvaluator;
pub use capability::CapabilityResolverImpl;
pub use data_product_monitor::RdfDataProductSLOMonitor;
pub use implementation::{ContentType, IngressRoute, IngressTable, PiCloudHttpServer, new_ingress_table, resource_response};
pub use inference::InferenceEngine;
pub use ingress::IngressEventHandler;
pub use metrics::MetricsAgent;
pub use otel::{OtelAggregator, OtelDatum, OtelLogRecord, OtelStream, aggregate_otel_metrics, parse_otlp_json};
pub use parquet_store::ParquetTelemetryStore;

/// Build the telemetry store used by the main `picloud-server` composition root
/// and by regression tests (TC-356). Mirrors exactly what `src/main.rs` wires
/// up so tests exercise the real backend.
///
/// Returns a concrete `Arc<ParquetTelemetryStore>` so callers can:
/// - Call inherent methods such as `start_retention_cleanup()`
/// - Clone into `Arc<dyn TelemetryStore>` via `.clone() as _`
///
/// The Parquet backend is the authoritative store defined by ADR-046 and is
/// the only backend that supports DataFusion SQL queries (`query_sql`).
pub fn build_main_telemetry_store(
    base_path: impl Into<std::path::PathBuf>,
    retention_hours: u64,
) -> std::sync::Arc<ParquetTelemetryStore> {
    std::sync::Arc::new(
        ParquetTelemetryStore::new(base_path).with_retention_hours(retention_hours),
    )
}
pub use provisioner::Provisioner;
pub use router::{IngressRouter, SharedRouter};
pub use self_monitor::PlatformSelfMonitor;
pub use telemetry_store::JsonlTelemetryStore;
pub use tls::{SharedTls, TlsState};
