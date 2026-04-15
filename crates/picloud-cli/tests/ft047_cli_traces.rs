//! FT-047 — CLI traces — every command produces an OTel trace
//!
//! Covers TC-260 (scenario) and TC-317 (exit-criteria).
//!
//! TC-260: CLI command produces OTel trace with correct span hierarchy.
//! TC-317: CLI traces exit — CLI commands produce OTel traces.
//!
//! These tests verify that:
//! 1. Every CLI command invocation creates a root span with a valid trace_id
//! 2. HTTP calls produce child spans parented to the root
//! 3. All spans share the same trace_id
//! 4. The OTLP payload produced is well-formed
//! 5. W3C traceparent header is correctly formatted

use picloud_cli::tracer::{self, CliTracer};

// ---------------------------------------------------------------------------
// TC-260 — CLI command produces OTel trace with correct span hierarchy
// ---------------------------------------------------------------------------

/// A CLI command produces a root span with a valid OTel trace ID (32 hex chars),
/// a valid span ID (16 hex chars), no parent, and service_name = "picloud-cli".
#[test]
fn tc260_cli_command_produces_otel_trace_with_correct_span_hierarchy() {
    let tracer = CliTracer::new("cluster status");

    // --- Root span ---
    let root = tracer.start_root_span("cluster status");
    assert!(
        root.parent_span_id.is_none(),
        "root span must have no parent"
    );
    assert_eq!(root.trace_id.len(), 32, "trace_id must be 32 hex chars");
    assert!(
        root.trace_id.chars().all(|c| c.is_ascii_hexdigit()),
        "trace_id must be valid hex"
    );
    assert_eq!(root.span_id.len(), 16, "span_id must be 16 hex chars");
    assert!(
        root.span_id.chars().all(|c| c.is_ascii_hexdigit()),
        "span_id must be valid hex"
    );
    assert_eq!(root.service_name, "picloud-cli");
    assert!(root.operation_name.contains("cluster status"));

    // --- Child spans (simulate HTTP calls) ---
    let child1 = tracer.start_child_span("GET /");
    assert_eq!(
        child1.parent_span_id.as_deref(),
        Some(tracer.root_span_id()),
        "child span must reference the root span as parent"
    );
    assert_eq!(
        child1.trace_id,
        root.trace_id,
        "child must share the same trace_id as root"
    );
    assert_ne!(
        child1.span_id, root.span_id,
        "child must have a different span_id than root"
    );

    let child2 = tracer.start_child_span("GET /nodes");
    assert_eq!(
        child2.parent_span_id.as_deref(),
        Some(tracer.root_span_id()),
        "second child also references root"
    );
    assert_eq!(child2.trace_id, root.trace_id);
    assert_ne!(child2.span_id, child1.span_id, "each child has a unique span_id");

    // --- Finish and collect ---
    let child1_record = child1.finish("OK");
    let child2_record = child2.finish("OK");
    let root_record = root.finish("OK");

    tracer.record(root_record.clone());
    tracer.record(child1_record.clone());
    tracer.record(child2_record.clone());

    let spans = tracer.spans();
    assert_eq!(spans.len(), 3, "should have root + 2 children");

    // All spans share the same trace_id
    let trace_id = &spans[0].trace_id;
    for span in &spans {
        assert_eq!(&span.trace_id, trace_id, "all spans must share one trace_id");
    }

    // Root has no parent, children point to root
    assert!(spans[0].parent_span_id.is_none());
    assert_eq!(spans[1].parent_span_id.as_deref(), Some(spans[0].span_id.as_str()));
    assert_eq!(spans[2].parent_span_id.as_deref(), Some(spans[0].span_id.as_str()));

    // Verify timing: end_time >= start_time for all spans
    for span in &spans {
        assert!(
            span.end_time >= span.start_time,
            "end_time must be >= start_time for span {}",
            span.operation_name
        );
    }

    // Verify the OTLP payload is well-formed
    let otlp = tracer::spans_to_otlp(&spans);
    let otlp_spans = otlp.get("spans").unwrap().as_array().unwrap();
    assert_eq!(otlp_spans.len(), 3);
    for s in otlp_spans {
        assert!(s.get("traceId").is_some());
        assert!(s.get("spanId").is_some());
        assert!(s.get("operationName").is_some());
        assert!(s.get("serviceName").is_some());
        assert!(s.get("startTimeUnixNano").is_some());
        assert!(s.get("endTimeUnixNano").is_some());
        assert!(s.get("status").is_some());
    }

    // The root span in OTLP should not have parentSpanId
    assert!(
        otlp_spans[0].get("parentSpanId").is_none(),
        "root span OTLP must not have parentSpanId"
    );
    // Children should have parentSpanId
    assert!(
        otlp_spans[1].get("parentSpanId").is_some(),
        "child span OTLP must have parentSpanId"
    );
    assert!(
        otlp_spans[2].get("parentSpanId").is_some(),
        "child span OTLP must have parentSpanId"
    );

    // W3C traceparent header is correctly formatted
    let tp = tracer.traceparent();
    let parts: Vec<&str> = tp.split('-').collect();
    assert_eq!(parts.len(), 4, "traceparent must have 4 parts");
    assert_eq!(parts[0], "00", "version must be 00");
    assert_eq!(parts[1].len(), 32, "trace-id must be 32 hex chars");
    assert_eq!(parts[2].len(), 16, "parent-id must be 16 hex chars");
    assert_eq!(parts[3], "01", "trace-flags must be 01 (sampled)");
}

// ---------------------------------------------------------------------------
// TC-317 — CLI traces exit — CLI commands produce OTel traces
// ---------------------------------------------------------------------------

/// Every CLI command type produces a valid OTel trace. Verify that the tracer
/// works correctly for various command names representing different CLI subcommands.
#[test]
fn tc317_cli_traces_exit_cli_commands_produce_otel_traces() {
    // Simulate multiple different CLI command types
    let commands = vec![
        "cluster init",
        "cluster status",
        "cluster recover",
        "resource apply",
        "resource delete",
        "resource status",
        "identity create",
        "identity token",
        "events stream",
        "graph query",
        "ca export",
        "sdk generate",
        "tag add",
        "alerts list",
        "telemetry query",
        "volume snapshots",
        "compile validate",
        "new product",
        "image push",
        "registry gc",
        "capability list",
        "data-domain list",
        "data-product list",
    ];

    for cmd in &commands {
        let tracer = CliTracer::new(cmd);

        // Every command gets a root span
        let root = tracer.start_root_span(cmd);
        assert!(
            root.parent_span_id.is_none(),
            "root span for '{}' must have no parent",
            cmd
        );
        assert_eq!(root.service_name, "picloud-cli");
        assert!(
            root.operation_name.contains(cmd),
            "'{}' must appear in operation_name '{}'",
            cmd,
            root.operation_name
        );

        // trace_id and span_id are valid
        assert_eq!(root.trace_id.len(), 32);
        assert!(root.trace_id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(root.span_id.len(), 16);
        assert!(root.span_id.chars().all(|c| c.is_ascii_hexdigit()));

        // Simulate an HTTP call child span
        let child = tracer.start_child_span(&format!("HTTP GET /{}", cmd.replace(' ', "/")));
        assert_eq!(child.trace_id, root.trace_id);
        assert_eq!(
            child.parent_span_id.as_deref(),
            Some(tracer.root_span_id())
        );

        // Finish and collect
        let child_record = child.finish("OK");
        tracer.record(child_record);
        let root_record = root.finish("OK");
        tracer.record(root_record);

        let spans = tracer.spans();
        assert_eq!(
            spans.len(),
            2,
            "command '{}' must produce exactly 2 spans (root + 1 child)",
            cmd
        );

        // OTLP payload is valid
        let otlp = tracer::spans_to_otlp(&spans);
        let otlp_spans = otlp.get("spans").unwrap().as_array().unwrap();
        assert_eq!(otlp_spans.len(), 2);
        for s in otlp_spans {
            assert!(s.get("traceId").is_some());
            assert!(s.get("spanId").is_some());
        }
    }

    // Verify command_name extraction utility
    assert_eq!(
        tracer::command_name(&["picloud".into(), "cluster".into(), "status".into()]),
        "cluster status"
    );
    assert_eq!(
        tracer::command_name(&["picloud".into(), "resource".into(), "apply".into(), "--path".into(), "/tmp".into()]),
        "resource apply"
    );
    assert_eq!(
        tracer::command_name(&["picloud".into(), "events".into(), "stream".into()]),
        "events stream"
    );
}

// ---------------------------------------------------------------------------
// Additional validation: traceparent uniqueness and trace isolation
// ---------------------------------------------------------------------------

/// Each CLI command invocation gets a unique trace_id — traces from different
/// commands never collide.
#[test]
fn tc260_traces_are_unique_per_invocation() {
    let tracer_a = CliTracer::new("cluster status");
    let tracer_b = CliTracer::new("cluster status");

    assert_ne!(
        tracer_a.trace_id(),
        tracer_b.trace_id(),
        "separate invocations must have different trace_ids"
    );
    assert_ne!(
        tracer_a.root_span_id(),
        tracer_b.root_span_id(),
        "separate invocations must have different root span_ids"
    );
    assert_ne!(
        tracer_a.traceparent(),
        tracer_b.traceparent(),
        "separate invocations must have different traceparent headers"
    );
}

/// Verify that the finish_root method records the root span correctly.
#[test]
fn tc260_finish_root_records_span() {
    let tracer = CliTracer::new("cluster status");
    let root = tracer.start_root_span("cluster status");
    tracer.finish_root(root, "OK");

    let spans = tracer.spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].status, "OK");
    assert_eq!(spans[0].service_name, "picloud-cli");
    assert!(spans[0].parent_span_id.is_none());
}

/// Verify error status propagation for failed commands.
#[test]
fn tc260_error_status_propagation() {
    let tracer = CliTracer::new("resource apply");
    let root = tracer.start_root_span("resource apply");

    // Simulate a failed HTTP call
    let child = tracer.start_child_span("POST /api/apply");
    tracer.record(child.finish("ERROR"));

    // Root span finishes with error too
    tracer.finish_root(root, "ERROR");

    let spans = tracer.spans();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].status, "ERROR"); // child
    assert_eq!(spans[1].status, "ERROR"); // root
}
