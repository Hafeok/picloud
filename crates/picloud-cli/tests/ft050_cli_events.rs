//! FT-050 — CLI events stream, graph query, identity token, telemetry query
//!
//! Covers TC-263 (scenario) and TC-320 (exit-criteria).
//!
//! TC-263: CLI events stream and graph query return expected results.
//!   Verifies URL path construction, SSE event parsing, graph result formatting,
//!   telemetry query paths, and identity token device-flow parsing.
//!
//! TC-320: CLI events exit — events stream and graph query functional.
//!   Exit-criteria test validating that all four command families (events, graph,
//!   identity token, telemetry) produce correct outputs across all parameter
//!   combinations.

use picloud_cli::commands;
use picloud_cli::commands::SseEvent;
use serde_json::json;

// ===========================================================================
// TC-263 — CLI events stream and graph query return expected results
// ===========================================================================

/// Events stream path with product and correlation_id builds the correct URL,
/// graph query path URL-encodes SPARQL, SSE events are parsed and formatted,
/// telemetry query paths handle all signal types, and identity token device-flow
/// JSON is parsed correctly.
#[test]
fn tc263_cli_events_stream_and_graph_query_return_expected_results() {
    // -----------------------------------------------------------------------
    // 1. Events stream path construction
    // -----------------------------------------------------------------------

    // No filters → global stream
    assert_eq!(
        commands::events_stream_path(None, None),
        "/api/events/stream"
    );

    // Product only
    assert_eq!(
        commands::events_stream_path(Some("photo-app"), None),
        "/products/photo-app/events"
    );

    // Product + correlation_id
    assert_eq!(
        commands::events_stream_path(Some("photo-app"), Some("abc-123")),
        "/products/photo-app/events?correlation_id=abc-123"
    );

    // correlation_id only (no product)
    assert_eq!(
        commands::events_stream_path(None, Some("abc-123")),
        "/api/events/stream?correlation_id=abc-123"
    );

    // -----------------------------------------------------------------------
    // 2. Graph query path construction
    // -----------------------------------------------------------------------

    // Global query — SPARQL is URL-encoded
    let sparql = "SELECT ?s ?p ?o WHERE { ?s ?p ?o }";
    let path = commands::graph_query_path(sparql, None);
    assert!(
        path.starts_with("/graph?query="),
        "global graph query must start with /graph?query="
    );
    // Verify spaces are encoded
    assert!(
        path.contains("%20"),
        "SPARQL spaces must be URL-encoded: {}",
        path
    );
    // Verify braces are encoded
    assert!(
        path.contains("%7B") && path.contains("%7D"),
        "SPARQL braces must be URL-encoded: {}",
        path
    );

    // Product-scoped query
    let path = commands::graph_query_path(sparql, Some("photo-app"));
    assert!(
        path.starts_with("/products/photo-app/graph?query="),
        "product graph query must route via /products/: {}",
        path
    );

    // -----------------------------------------------------------------------
    // 3. SSE event parsing
    // -----------------------------------------------------------------------

    // Valid JSON data line
    let event_json = json!({
        "event_type": "ResourceReady",
        "source": "https://picloud.local/products/photo-app/containers/api",
        "timestamp": "2026-04-15T12:00:00Z",
        "payload": { "status": "running" }
    });
    let line = format!("data: {}", serde_json::to_string(&event_json).unwrap());
    match commands::parse_sse_line(&line) {
        SseEvent::Data(parsed) => {
            assert_eq!(
                parsed.get("event_type").and_then(|v| v.as_str()),
                Some("ResourceReady")
            );
            assert_eq!(
                parsed.get("source").and_then(|v| v.as_str()),
                Some("https://picloud.local/products/photo-app/containers/api")
            );
        }
        other => panic!("expected SseEvent::Data, got {:?}", other),
    }

    // Raw (non-JSON) data line
    match commands::parse_sse_line("data: heartbeat") {
        SseEvent::RawData(text) => assert_eq!(text, "heartbeat"),
        other => panic!("expected SseEvent::RawData, got {:?}", other),
    }

    // Event type line
    match commands::parse_sse_line("event: ResourceReady") {
        SseEvent::EventType(name) => assert_eq!(name, "ResourceReady"),
        other => panic!("expected SseEvent::EventType, got {:?}", other),
    }

    // Empty / comment lines
    assert_eq!(commands::parse_sse_line(""), SseEvent::Ignored);
    assert_eq!(commands::parse_sse_line(": keepalive"), SseEvent::Ignored);

    // -----------------------------------------------------------------------
    // 4. SSE event formatting
    // -----------------------------------------------------------------------

    let formatted = commands::format_sse_event(&event_json);
    assert!(
        formatted.contains("[2026-04-15T12:00:00Z]"),
        "formatted output must contain timestamp"
    );
    assert!(
        formatted.contains("ResourceReady"),
        "formatted output must contain event type"
    );
    assert!(
        formatted.contains("https://picloud.local/products/photo-app/containers/api"),
        "formatted output must contain source IRI"
    );
    assert!(
        formatted.contains("running"),
        "formatted output must contain payload content"
    );

    // -----------------------------------------------------------------------
    // 5. Graph result formatting
    // -----------------------------------------------------------------------

    let graph_body = json!({
        "results": {
            "bindings": [
                { "s": { "value": "https://picloud.local/products/photo-app" } }
            ]
        }
    });
    let formatted = commands::format_graph_results(&graph_body);
    assert!(
        formatted.contains("photo-app"),
        "graph results must contain the binding value"
    );
    // Must be valid JSON (pretty-printed)
    let reparsed: serde_json::Value = serde_json::from_str(&formatted)
        .expect("formatted graph results must be valid JSON");
    assert_eq!(reparsed, graph_body);

    // -----------------------------------------------------------------------
    // 6. Telemetry query path construction
    // -----------------------------------------------------------------------

    // Traces with no filters
    assert_eq!(
        commands::telemetry_query_path("traces", None, None, None),
        Some("/telemetry/spans".to_string())
    );

    // Spans alias
    assert_eq!(
        commands::telemetry_query_path("spans", None, None, None),
        Some("/telemetry/spans".to_string())
    );

    // Metrics signal
    assert_eq!(
        commands::telemetry_query_path("metrics", None, None, None),
        Some("/telemetry/metrics".to_string())
    );

    // Unknown signal returns None
    assert!(
        commands::telemetry_query_path("logs", None, None, None).is_none(),
        "unknown signal type must return None"
    );

    // Traces with all filters
    let path = commands::telemetry_query_path(
        "traces",
        Some("2026-04-01T00:00:00Z"),
        Some("2026-04-15T00:00:00Z"),
        Some("picloud-cli"),
    )
    .unwrap();
    assert!(path.starts_with("/telemetry/spans?"));
    assert!(path.contains("from="));
    assert!(path.contains("to="));
    assert!(path.contains("service=picloud-cli"));

    // -----------------------------------------------------------------------
    // 7. Telemetry SQL body construction
    // -----------------------------------------------------------------------

    let body = commands::telemetry_sql_body("traces", "SELECT * FROM spans LIMIT 10");
    assert_eq!(body["signal"], "traces");
    assert_eq!(body["sql"], "SELECT * FROM spans LIMIT 10");

    // -----------------------------------------------------------------------
    // 8. Identity token — device flow begin parsing
    // -----------------------------------------------------------------------

    let flow_resp = json!({
        "device_code": "ABCD-1234",
        "verification_url": "http://picloud.local:7443/auth/device/verify",
        "interval_secs": 3,
        "expires_in_secs": 300
    });
    let (device_code, verification_url, interval, expires_in) =
        commands::parse_device_flow_begin(&flow_resp);
    assert_eq!(device_code, "ABCD-1234");
    assert_eq!(
        verification_url,
        "http://picloud.local:7443/auth/device/verify"
    );
    assert_eq!(interval, 3);
    assert_eq!(expires_in, 300);

    // Defaults for missing fields
    let empty_resp = json!({});
    let (dc, vu, iv, ex) = commands::parse_device_flow_begin(&empty_resp);
    assert_eq!(dc, "");
    assert_eq!(vu, "");
    assert_eq!(iv, 5);  // default
    assert_eq!(ex, 600); // default

    // -----------------------------------------------------------------------
    // 9. Identity token — device flow poll status parsing
    // -----------------------------------------------------------------------

    assert_eq!(
        commands::parse_device_flow_poll_status(&json!({"status": "complete", "access_token": "tok123"})),
        "complete"
    );
    assert_eq!(
        commands::parse_device_flow_poll_status(&json!({"status": "pending"})),
        "pending"
    );
    assert_eq!(
        commands::parse_device_flow_poll_status(&json!({"status": "expired"})),
        "expired"
    );
    assert_eq!(
        commands::parse_device_flow_poll_status(&json!({})),
        "unknown"
    );

    // -----------------------------------------------------------------------
    // 10. Identity token — access token extraction
    // -----------------------------------------------------------------------

    let complete_resp = json!({"status": "complete", "access_token": "eyJhbGciOiJFZDI1NTE5In0.test"});
    assert_eq!(
        commands::extract_access_token(&complete_resp),
        "eyJhbGciOiJFZDI1NTE5In0.test"
    );
    assert_eq!(commands::extract_access_token(&json!({})), "");
}

// ===========================================================================
// TC-320 — CLI events exit — events stream and graph query functional
// ===========================================================================

/// Exit criteria: all four CLI command families (events stream, graph query,
/// identity token, telemetry query) produce correct URL paths, parse responses
/// correctly, and handle edge cases across all parameter combinations.
#[test]
fn tc320_cli_events_exit_events_stream_and_graph_query_functional() {
    // -----------------------------------------------------------------------
    // A. Events stream — all parameter combinations
    // -----------------------------------------------------------------------
    let event_cases: Vec<(Option<&str>, Option<&str>, &str)> = vec![
        (None, None, "/api/events/stream"),
        (Some("photo-app"), None, "/products/photo-app/events"),
        (
            Some("photo-app"),
            Some("corr-1"),
            "/products/photo-app/events?correlation_id=corr-1",
        ),
        (None, Some("corr-2"), "/api/events/stream?correlation_id=corr-2"),
        // Edge: product with special characters in name
        (Some("my-app-2"), None, "/products/my-app-2/events"),
    ];

    for (product, correlation_id, expected) in &event_cases {
        assert_eq!(
            commands::events_stream_path(*product, *correlation_id),
            *expected,
            "events_stream_path({:?}, {:?})",
            product,
            correlation_id
        );
    }

    // -----------------------------------------------------------------------
    // B. Graph query — all parameter combinations
    // -----------------------------------------------------------------------

    // Various SPARQL queries
    let queries = vec![
        "SELECT ?s WHERE { ?s a <http://schema.org/Product> }",
        "ASK { <https://picloud.local/products/x> ?p ?o }",
        "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o } LIMIT 10",
    ];

    for sparql in &queries {
        // Global
        let path = commands::graph_query_path(sparql, None);
        assert!(
            path.starts_with("/graph?query="),
            "global path for '{}': {}",
            sparql,
            path
        );

        // Product-scoped
        let path = commands::graph_query_path(sparql, Some("test-product"));
        assert!(
            path.starts_with("/products/test-product/graph?query="),
            "product path for '{}': {}",
            sparql,
            path
        );
    }

    // Verify URL encoding is applied for special chars
    let sparql_with_specials = "SELECT ?x WHERE { ?x ?p \"hello world\" }";
    let path = commands::graph_query_path(sparql_with_specials, None);
    assert!(
        !path.contains(' '),
        "spaces must be encoded in query path: {}",
        path
    );

    // -----------------------------------------------------------------------
    // C. Identity token — device flow round-trip
    // -----------------------------------------------------------------------

    // Simulate a full device flow: begin → poll(pending) → poll(complete)
    let begin = json!({
        "device_code": "DEV-42",
        "verification_url": "https://picloud.local:7443/auth/device/verify?code=DEV-42",
        "interval_secs": 2,
        "expires_in_secs": 120
    });
    let (dc, vu, iv, ex) = commands::parse_device_flow_begin(&begin);
    assert_eq!(dc, "DEV-42");
    assert!(vu.contains("DEV-42"));
    assert_eq!(iv, 2);
    assert_eq!(ex, 120);

    // Pending poll
    let pending = json!({"status": "pending"});
    assert_eq!(commands::parse_device_flow_poll_status(&pending), "pending");
    assert_eq!(commands::extract_access_token(&pending), "");

    // Complete poll
    let complete = json!({
        "status": "complete",
        "access_token": "ey.TOKEN.SIG"
    });
    assert_eq!(commands::parse_device_flow_poll_status(&complete), "complete");
    assert_eq!(commands::extract_access_token(&complete), "ey.TOKEN.SIG");

    // Expired poll
    let expired = json!({"status": "expired"});
    assert_eq!(commands::parse_device_flow_poll_status(&expired), "expired");

    // -----------------------------------------------------------------------
    // D. Telemetry query — all signal types and filter combos
    // -----------------------------------------------------------------------

    // All valid signal types
    for signal in &["traces", "spans", "metrics"] {
        assert!(
            commands::telemetry_query_path(signal, None, None, None).is_some(),
            "signal '{}' must be recognized",
            signal
        );
    }

    // Invalid signal types
    for bad_signal in &["logs", "events", "spans_v2", ""] {
        assert!(
            commands::telemetry_query_path(bad_signal, None, None, None).is_none(),
            "signal '{}' must be rejected",
            bad_signal
        );
    }

    // Various filter combinations
    let filter_cases: Vec<(Option<&str>, Option<&str>, Option<&str>)> = vec![
        (None, None, None),
        (Some("2026-01-01T00:00:00Z"), None, None),
        (None, Some("2026-12-31T23:59:59Z"), None),
        (None, None, Some("picloud-http")),
        (
            Some("2026-01-01T00:00:00Z"),
            Some("2026-12-31T23:59:59Z"),
            Some("picloud-cli"),
        ),
    ];

    for (from, to, service) in &filter_cases {
        let path = commands::telemetry_query_path("traces", *from, *to, *service)
            .expect("traces must be recognized");
        assert!(
            path.starts_with("/telemetry/spans"),
            "traces path must start with /telemetry/spans: {}",
            path
        );
        if from.is_some() {
            assert!(path.contains("from="), "must contain from= param: {}", path);
        }
        if to.is_some() {
            assert!(path.contains("to="), "must contain to= param: {}", path);
        }
        if service.is_some() {
            assert!(
                path.contains("service="),
                "must contain service= param: {}",
                path
            );
        }
    }

    // SQL body is always well-formed
    let sql_body = commands::telemetry_sql_body(
        "traces",
        "SELECT operation_name, avg(duration_ms) FROM spans GROUP BY operation_name",
    );
    assert_eq!(sql_body["signal"], "traces");
    assert!(sql_body["sql"].as_str().unwrap().contains("GROUP BY"));

    // -----------------------------------------------------------------------
    // E. SSE parsing — multiple event types in sequence
    // -----------------------------------------------------------------------

    let lines = vec![
        ": keepalive",
        "event: ResourceDeclared",
        "data: {\"event_type\":\"ResourceDeclared\",\"source\":\"https://picloud.local/products/x/containers/a\",\"timestamp\":\"2026-04-15T10:00:00Z\",\"payload\":{}}",
        "",
        "event: ResourceReady",
        "data: {\"event_type\":\"ResourceReady\",\"source\":\"https://picloud.local/products/x/containers/a\",\"timestamp\":\"2026-04-15T10:00:01Z\",\"payload\":{\"status\":\"running\"}}",
        "",
        "data: not-json-heartbeat",
    ];

    let mut data_events = 0;
    let mut event_type_events = 0;
    let mut raw_events = 0;

    for line in &lines {
        match commands::parse_sse_line(line) {
            SseEvent::Data(_) => data_events += 1,
            SseEvent::EventType(_) => event_type_events += 1,
            SseEvent::RawData(_) => raw_events += 1,
            SseEvent::Ignored => {}
        }
    }

    assert_eq!(data_events, 2, "should parse 2 JSON data events");
    assert_eq!(event_type_events, 2, "should parse 2 event type lines");
    assert_eq!(raw_events, 1, "should parse 1 raw data event");

    // -----------------------------------------------------------------------
    // F. URL encoding — special characters
    // -----------------------------------------------------------------------

    assert_eq!(commands::urlencoding("hello world"), "hello%20world");
    assert_eq!(commands::urlencoding("a&b"), "a%26b");
    assert_eq!(commands::urlencoding("{x}"), "%7Bx%7D");
    assert_eq!(commands::urlencoding("a?b#c"), "a%3Fb%23c");

    // -----------------------------------------------------------------------
    // G. SQL WHERE clause parsing (used by telemetry)
    // -----------------------------------------------------------------------

    let clauses = commands::parse_sql_where_clause(
        "SELECT * FROM traces WHERE service = 'picloud-cli' AND duration_ms > 50",
    );
    assert_eq!(clauses.len(), 2);
    assert_eq!(clauses[0].0, "service");
    assert_eq!(clauses[0].1, "picloud-cli");
    assert_eq!(clauses[1].0, "duration_ms");
    assert_eq!(clauses[1].1, "50");

    // No WHERE clause
    assert!(commands::parse_sql_where_clause("SELECT * FROM spans").is_empty());
}
