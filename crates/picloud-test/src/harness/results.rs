/// Result reporting — ADR-057 (OpenTelemetry test data versioning).
///
/// Exports scenario results as OTel spans with platform version attributes
/// resolved via DNS TXT records or cluster.toml fallback.
use std::time::Duration;

use opentelemetry::trace::{Span, SpanKind, Status, Tracer, TracerProvider as _};
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_sdk::Resource;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::ClusterConfig;
use crate::harness::runner::ScenarioResult;

// ---------------------------------------------------------------------------
// DNS TXT-based platform version resolution
// ---------------------------------------------------------------------------

/// Resolve the platform version for OTel resource attributes.
///
/// Strategy:
/// 1. If `config.cluster.platform_version` is a concrete semver, use it.
/// 2. If it is `"auto"`, query the cluster DNS for a TXT record on the
///    cluster domain and parse `platform-version=X.Y.Z`.
/// 3. Fall back to `"unknown"` if DNS lookup fails.
pub async fn resolve_platform_version(config: &ClusterConfig) -> String {
    let configured = &config.cluster.platform_version;

    // If an explicit version was set (not "auto"), use it directly.
    if configured != "auto" {
        return configured.clone();
    }

    // Try DNS TXT record lookup.
    match lookup_dns_txt_version(&config.cluster.domain).await {
        Some(v) => {
            info!(version = v.as_str(), "resolved platform version via DNS TXT");
            v
        }
        None => {
            warn!(
                domain = config.cluster.domain.as_str(),
                "DNS TXT lookup for platform version failed, using 'unknown'"
            );
            "unknown".to_string()
        }
    }
}

/// Query DNS TXT records for `domain` and extract `platform-version=...`.
async fn lookup_dns_txt_version(domain: &str) -> Option<String> {
    use hickory_resolver::Resolver;

    // Build a resolver using the default tokio provider.
    let resolver = match Resolver::builder_tokio() {
        Ok(builder) => builder.build(),
        Err(e) => {
            warn!(error = %e, "failed to build DNS resolver");
            return None;
        }
    };

    let lookup = match resolver.txt_lookup(domain).await {
        Ok(l) => l,
        Err(e) => {
            warn!(domain, error = %e, "DNS TXT lookup failed");
            return None;
        }
    };

    for txt_record in lookup.iter() {
        let txt: String = txt_record.to_string();
        // TXT records may contain multiple key=value pairs separated by spaces
        // or be individual records. Look for the platform-version key.
        for part in txt.split_whitespace() {
            if let Some(version) = part.strip_prefix("platform-version=") {
                let version = version.trim().to_string();
                if !version.is_empty() {
                    return Some(version);
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// OTel tracer initialization
// ---------------------------------------------------------------------------

/// Handle returned by `init_test_tracer` for use throughout the test run.
pub struct OtelTestContext {
    pub tracer_provider: TracerProvider,
    pub run_id: String,
    pub platform_version: String,
    pub suite_name: String,
}

impl OtelTestContext {
    /// Obtain the named tracer from the provider.
    pub fn tracer(&self) -> opentelemetry_sdk::trace::Tracer {
        self.tracer_provider.tracer("picloud-test")
    }
}

/// Initialise the OTel tracer for the test run.
///
/// Creates a `TracerProvider` with resource attributes per ADR-057:
/// - `service.name` = "picloud-test"
/// - `service.version` = crate version
/// - `picloud.platform_version` = resolved version
/// - `picloud.test.run_id` = UUID v4
/// - `picloud.test.suite` = suite name
/// - `picloud.test.cluster` = cluster domain
///
/// The OTLP exporter targets `https://{cluster_domain}:4317` by default,
/// overridable via `OTEL_EXPORTER_OTLP_ENDPOINT`.
///
/// Returns `None` (with a warning) if the exporter cannot be built, so that
/// tests still run without OTel infrastructure.
pub async fn init_test_tracer(
    config: &ClusterConfig,
    suite_name: &str,
) -> Option<OtelTestContext> {
    let platform_version = resolve_platform_version(config).await;
    let run_id = Uuid::new_v4().to_string();

    let resource = Resource::new(vec![
        KeyValue::new("service.name", "picloud-test"),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("picloud.platform_version", platform_version.clone()),
        KeyValue::new("picloud.test.run_id", run_id.clone()),
        KeyValue::new("picloud.test.suite", suite_name.to_string()),
        KeyValue::new("picloud.test.cluster", config.cluster.domain.clone()),
    ]);

    // Build OTLP exporter — use env var or fall back to cluster endpoint.
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").unwrap_or_else(|_| {
        format!("https://{}:4317", config.cluster.domain)
    });

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
    {
        Ok(e) => e,
        Err(e) => {
            warn!(
                endpoint = endpoint.as_str(),
                error = %e,
                "failed to build OTLP exporter — OTel disabled for this run"
            );
            return None;
        }
    };

    let provider = TracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build();

    info!(
        run_id = run_id.as_str(),
        platform_version = platform_version.as_str(),
        endpoint = endpoint.as_str(),
        "OTel tracer initialised"
    );

    Some(OtelTestContext {
        tracer_provider: provider,
        run_id,
        platform_version,
        suite_name: suite_name.to_string(),
    })
}

/// Shut down the OTel tracer provider, flushing pending spans.
pub fn shutdown_tracer(ctx: OtelTestContext) {
    if let Err(e) = ctx.tracer_provider.shutdown() {
        warn!(error = %e, "OTel tracer shutdown error");
    }
}

// ---------------------------------------------------------------------------
// Scenario result recording as OTel spans
// ---------------------------------------------------------------------------

/// Start an OTel span for a scenario. Returns the span to be ended later.
pub fn record_scenario_start(
    otel: &OtelTestContext,
    name: &str,
    adr: &str,
) -> opentelemetry::trace::SpanRef<'static> {
    // We can't return SpanRef directly because of lifetime issues.
    // Instead, use the lower-level API via the tracer.
    // Callers should use `record_scenario` which wraps the full lifecycle.
    let _ = (otel, name, adr);
    unimplemented!("use record_scenario() instead for the full lifecycle")
}

/// Record a complete scenario execution as an OTel span.
///
/// This is the preferred API — it creates a span, sets attributes based on
/// the scenario result, and ends it immediately.
pub fn record_scenario(otel: &OtelTestContext, name: &str, adr: &str, result: &ScenarioResult) {
    let tracer = otel.tracer();

    let mut span = tracer
        .span_builder(format!("scenario:{}", name))
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("picloud.test.scenario.name", name.to_string()),
            KeyValue::new("picloud.test.scenario.adr", adr.to_string()),
            KeyValue::new("picloud.test.run_id", otel.run_id.clone()),
        ])
        .start(&tracer);

    match result {
        ScenarioResult::Pass { duration } => {
            span.set_attribute(KeyValue::new("picloud.test.scenario.status", "pass"));
            span.set_attribute(KeyValue::new(
                "picloud.test.scenario.duration_ms",
                duration.as_millis() as i64,
            ));
            span.set_status(Status::Ok);
        }
        ScenarioResult::Fail { duration, reason } => {
            span.set_attribute(KeyValue::new("picloud.test.scenario.status", "fail"));
            span.set_attribute(KeyValue::new(
                "picloud.test.scenario.duration_ms",
                duration.as_millis() as i64,
            ));
            span.set_attribute(KeyValue::new(
                "picloud.test.scenario.failure_reason",
                reason.clone(),
            ));
            span.set_status(Status::error(reason.clone()));
        }
        ScenarioResult::Skip { reason } => {
            span.set_attribute(KeyValue::new("picloud.test.scenario.status", "skip"));
            span.set_attribute(KeyValue::new(
                "picloud.test.scenario.skip_reason",
                reason.clone(),
            ));
            span.set_status(Status::Unset);
        }
    }

    span.end();
}

// ---------------------------------------------------------------------------
// Run summary (unchanged from original)
// ---------------------------------------------------------------------------

/// Summary of a complete test run.
pub struct RunSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration: Duration,
}

impl RunSummary {
    pub fn new(passed: usize, failed: usize, skipped: usize, total_duration: Duration) -> Self {
        Self {
            passed,
            failed,
            skipped,
            total_duration,
        }
    }

    pub fn total(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }

    /// Print a human-readable summary to stdout.
    pub fn print(&self) {
        println!();
        println!("=== Test Run Summary ===");
        println!(
            "  Total: {}  Passed: {}  Failed: {}  Skipped: {}",
            self.total(),
            self.passed,
            self.failed,
            self.skipped
        );
        println!("  Duration: {:.2}s", self.total_duration.as_secs_f64());
        if self.all_passed() {
            println!("  Result: ALL PASSED");
        } else {
            println!("  Result: {} FAILURE(S)", self.failed);
        }
        println!("========================");
    }
}
