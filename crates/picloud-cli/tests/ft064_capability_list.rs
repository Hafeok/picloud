//! FT-064 — picloud capability list — all capabilities, implementors, consumers,
//! and fulfilment status
//!
//! Covers TC-271 (scenario) and TC-328 (exit-criteria).
//!
//! TC-271: picloud capability list shows all capabilities with fulfilment status.
//!   Verifies that the capability list SPARQL query is correctly constructed,
//!   response parsing extracts implementors/consumers/fulfilment, and the table
//!   output includes all required columns.
//!
//! TC-328: Capability list exit — all capabilities shown with fulfilment status.
//!   Exit-criteria test validating that capability list handles empty results,
//!   multiple capabilities, mixed fulfilment states, and edge cases correctly.

use picloud_cli::commands;
use picloud_cli::commands::CapabilityListRow;
use serde_json::json;

// ===========================================================================
// TC-271 — picloud capability list shows all capabilities with fulfilment status
// ===========================================================================

/// The capability list SPARQL query fetches name, version, status, implementors,
/// and consumers. Parsing extracts all fields correctly, fulfilment is derived
/// from implementors presence, and the table output includes all columns.
#[test]
fn tc271_picloud_capability_list_shows_all_capabilities_with_fulfilment_status() {
    // -----------------------------------------------------------------------
    // 1. SPARQL query construction
    // -----------------------------------------------------------------------

    let sparql = commands::capability_list_sparql();

    // Must query for capability type
    assert!(
        sparql.contains("rdf:type picloud:Capability")
            || sparql.contains("a picloud:Capability"),
        "SPARQL must select Capability type: {}",
        sparql
    );

    // Must select name, version
    assert!(
        sparql.contains("picloud:name ?name"),
        "SPARQL must select name: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:version ?version"),
        "SPARQL must select version: {}",
        sparql
    );

    // Must aggregate implementors and consumers
    assert!(
        sparql.contains("picloud:implementedBy"),
        "SPARQL must query implementors: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:consumedBy"),
        "SPARQL must query consumers: {}",
        sparql
    );
    assert!(
        sparql.contains("GROUP_CONCAT"),
        "SPARQL must use GROUP_CONCAT for aggregation: {}",
        sparql
    );

    // -----------------------------------------------------------------------
    // 2. Response parsing — fulfilled capability (has implementors)
    // -----------------------------------------------------------------------

    let body = json!({
        "bindings": [
            {
                "name": { "value": "image-resize" },
                "version": { "value": "1.2.0" },
                "status": { "value": "ready" },
                "implementors": { "value": "https://picloud.local/products/photo-app" },
                "consumers": { "value": "https://picloud.local/products/gallery-app" }
            }
        ]
    });

    let rows = commands::parse_capability_list(&body);
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.name, "image-resize");
    assert_eq!(row.version, "1.2.0");
    assert!(row.fulfilled, "capability with implementors must be fulfilled");
    assert_eq!(row.implementors, vec!["photo-app"]);
    assert_eq!(row.consumers, vec!["gallery-app"]);

    // -----------------------------------------------------------------------
    // 3. Response parsing — unfulfilled capability (no implementors)
    // -----------------------------------------------------------------------

    let body_unfulfilled = json!({
        "bindings": [
            {
                "name": { "value": "video-transcode" },
                "version": { "value": "1.0.0" },
                "status": { "value": "declared" },
                "implementors": { "value": "" },
                "consumers": { "value": "https://picloud.local/products/streaming-app" }
            }
        ]
    });

    let rows = commands::parse_capability_list(&body_unfulfilled);
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.name, "video-transcode");
    assert!(!row.fulfilled, "capability without implementors must not be fulfilled");
    assert!(row.implementors.is_empty());
    assert_eq!(row.consumers, vec!["streaming-app"]);

    // -----------------------------------------------------------------------
    // 4. Fulfilment logic
    // -----------------------------------------------------------------------

    assert!(
        commands::capability_is_fulfilled(&["photo-app".to_string()]),
        "non-empty implementors → fulfilled"
    );
    assert!(
        !commands::capability_is_fulfilled(&[]),
        "empty implementors → not fulfilled"
    );

    // -----------------------------------------------------------------------
    // 5. Table formatting includes all required columns
    // -----------------------------------------------------------------------

    let rows = vec![CapabilityListRow {
        name: "image-resize".to_string(),
        version: "1.2.0".to_string(),
        fulfilled: true,
        implementors: vec!["photo-app".to_string()],
        consumers: vec!["gallery-app".to_string()],
    }];

    let table = commands::format_capability_table(&rows);

    // Header columns
    assert!(table.contains("NAME"), "table must have NAME column");
    assert!(table.contains("VERSION"), "table must have VERSION column");
    assert!(
        table.contains("FULFILLED"),
        "table must have FULFILLED column"
    );
    assert!(
        table.contains("IMPLEMENTORS"),
        "table must have IMPLEMENTORS column"
    );
    assert!(
        table.contains("CONSUMERS"),
        "table must have CONSUMERS column"
    );

    // Row data
    assert!(
        table.contains("image-resize"),
        "table must show capability name"
    );
    assert!(table.contains("1.2.0"), "table must show version");
    assert!(
        table.contains("yes"),
        "table must show 'yes' for fulfilled capability"
    );
    assert!(
        table.contains("photo-app"),
        "table must show implementor name"
    );
    assert!(
        table.contains("gallery-app"),
        "table must show consumer name"
    );
}

// ===========================================================================
// TC-328 — Capability list exit — all capabilities shown with fulfilment status
// ===========================================================================

/// Exit criteria: capability list handles empty results, multiple capabilities,
/// mixed fulfilment states, multiple implementors/consumers, nested JSON
/// response format, and product name extraction from IRIs.
#[test]
fn tc328_capability_list_exit_all_capabilities_shown_with_fulfilment_status() {
    // -----------------------------------------------------------------------
    // A. Empty result set
    // -----------------------------------------------------------------------

    let empty_body = json!({ "bindings": [] });
    let rows = commands::parse_capability_list(&empty_body);
    assert!(rows.is_empty(), "empty bindings must produce no rows");

    let table = commands::format_capability_table(&rows);
    assert_eq!(
        table, "No capabilities declared.",
        "empty table must show placeholder message"
    );

    // -----------------------------------------------------------------------
    // B. Multiple capabilities with mixed fulfilment
    // -----------------------------------------------------------------------

    let body = json!({
        "bindings": [
            {
                "name": { "value": "image-resize" },
                "version": { "value": "2.0.0" },
                "status": { "value": "ready" },
                "implementors": { "value": "https://picloud.local/products/photo-app" },
                "consumers": { "value": "https://picloud.local/products/gallery-app, https://picloud.local/products/social-app" }
            },
            {
                "name": { "value": "video-transcode" },
                "version": { "value": "1.0.0" },
                "status": { "value": "declared" },
                "implementors": { "value": "" },
                "consumers": { "value": "" }
            },
            {
                "name": { "value": "pdf-render" },
                "version": { "value": "1.1.0" },
                "status": { "value": "ready" },
                "implementors": { "value": "https://picloud.local/products/doc-service, https://picloud.local/products/report-service" },
                "consumers": { "value": "https://picloud.local/products/admin-portal" }
            }
        ]
    });

    let rows = commands::parse_capability_list(&body);
    assert_eq!(rows.len(), 3, "must parse all three capabilities");

    // image-resize: fulfilled, 1 implementor, 2 consumers
    assert_eq!(rows[0].name, "image-resize");
    assert_eq!(rows[0].version, "2.0.0");
    assert!(rows[0].fulfilled);
    assert_eq!(rows[0].implementors, vec!["photo-app"]);
    assert_eq!(
        rows[0].consumers,
        vec!["gallery-app", "social-app"]
    );

    // video-transcode: unfulfilled, no implementors, no consumers
    assert_eq!(rows[1].name, "video-transcode");
    assert!(!rows[1].fulfilled);
    assert!(rows[1].implementors.is_empty());
    assert!(rows[1].consumers.is_empty());

    // pdf-render: fulfilled, 2 implementors, 1 consumer
    assert_eq!(rows[2].name, "pdf-render");
    assert!(rows[2].fulfilled);
    assert_eq!(
        rows[2].implementors,
        vec!["doc-service", "report-service"]
    );
    assert_eq!(rows[2].consumers, vec!["admin-portal"]);

    // Table must show all capabilities
    let table = commands::format_capability_table(&rows);
    assert!(table.contains("image-resize"), "must list image-resize");
    assert!(table.contains("video-transcode"), "must list video-transcode");
    assert!(table.contains("pdf-render"), "must list pdf-render");
    assert!(
        table.contains("yes") && table.contains("no"),
        "must show both fulfilled and unfulfilled states"
    );

    // -----------------------------------------------------------------------
    // C. Unfulfilled capability shows 'no' and '-' for implementors
    // -----------------------------------------------------------------------

    let unfulfilled_rows = vec![CapabilityListRow {
        name: "orphan-cap".to_string(),
        version: "0.1.0".to_string(),
        fulfilled: false,
        implementors: vec![],
        consumers: vec!["some-product".to_string()],
    }];

    let table = commands::format_capability_table(&unfulfilled_rows);
    assert!(
        table.contains("no"),
        "unfulfilled must show 'no': {}",
        table
    );
    assert!(
        table.contains("-"),
        "empty implementors must show '-': {}",
        table
    );
    assert!(
        table.contains("some-product"),
        "consumer must still show: {}",
        table
    );

    // -----------------------------------------------------------------------
    // D. Nested results format (results → bindings)
    // -----------------------------------------------------------------------

    let nested_body = json!({
        "results": {
            "bindings": [
                {
                    "name": { "value": "auth-check" },
                    "version": { "value": "1.0.0" },
                    "status": { "value": "ready" },
                    "implementors": { "value": "https://picloud.local/products/iam-svc" },
                    "consumers": { "value": "" }
                }
            ]
        }
    });

    let rows = commands::parse_capability_list(&nested_body);
    assert_eq!(rows.len(), 1, "must parse from nested results.bindings");
    assert_eq!(rows[0].name, "auth-check");
    assert!(rows[0].fulfilled);
    assert_eq!(rows[0].implementors, vec!["iam-svc"]);
    assert!(rows[0].consumers.is_empty());

    // -----------------------------------------------------------------------
    // E. Missing body structure returns empty
    // -----------------------------------------------------------------------

    let bad_body = json!({ "unexpected": "format" });
    let rows = commands::parse_capability_list(&bad_body);
    assert!(rows.is_empty(), "non-matching body must return empty list");

    let null_body = json!(null);
    let rows = commands::parse_capability_list(&null_body);
    assert!(rows.is_empty(), "null body must return empty list");

    // -----------------------------------------------------------------------
    // F. Product name extraction from various IRI formats
    // -----------------------------------------------------------------------

    // Standard IRI → short name
    let body_short = json!({
        "bindings": [{
            "name": { "value": "test-cap" },
            "version": { "value": "1.0.0" },
            "status": { "value": "ready" },
            "implementors": { "value": "https://picloud.local/products/my-svc" },
            "consumers": { "value": "https://picloud.local/products/my-consumer" }
        }]
    });
    let rows = commands::parse_capability_list(&body_short);
    assert_eq!(rows[0].implementors, vec!["my-svc"]);
    assert_eq!(rows[0].consumers, vec!["my-consumer"]);

    // Plain name (no IRI structure) passes through
    let body_plain = json!({
        "bindings": [{
            "name": { "value": "plain-cap" },
            "version": { "value": "1.0.0" },
            "status": { "value": "ready" },
            "implementors": { "value": "just-a-name" },
            "consumers": { "value": "" }
        }]
    });
    let rows = commands::parse_capability_list(&body_plain);
    assert_eq!(
        rows[0].implementors,
        vec!["just-a-name"],
        "plain names must pass through unchanged"
    );

    // -----------------------------------------------------------------------
    // G. SPARQL query is well-formed
    // -----------------------------------------------------------------------

    let sparql = commands::capability_list_sparql();

    // Must order results
    assert!(
        sparql.contains("ORDER BY"),
        "SPARQL must include ORDER BY: {}",
        sparql
    );

    // Must group results
    assert!(
        sparql.contains("GROUP BY"),
        "SPARQL must include GROUP BY: {}",
        sparql
    );

    // Must use OPTIONAL for implementors and consumers (they may be absent)
    assert!(
        sparql.contains("OPTIONAL"),
        "SPARQL must use OPTIONAL for nullable fields: {}",
        sparql
    );

    // URL encoding round-trip: SPARQL can be encoded without losing structure
    let encoded = commands::urlencoding(sparql);
    assert!(
        !encoded.contains(' '),
        "encoded SPARQL must not contain literal spaces"
    );
    assert!(
        encoded.contains("%7B") && encoded.contains("%7D"),
        "braces must be encoded"
    );

    // -----------------------------------------------------------------------
    // H. Table separator line
    // -----------------------------------------------------------------------

    let single_row = vec![CapabilityListRow {
        name: "x".to_string(),
        version: "1.0.0".to_string(),
        fulfilled: true,
        implementors: vec!["svc".to_string()],
        consumers: vec![],
    }];
    let table = commands::format_capability_table(&single_row);
    let lines: Vec<&str> = table.lines().collect();
    assert!(lines.len() >= 3, "table must have header, separator, and data rows");
    assert!(
        lines[1].chars().all(|c| c == '-'),
        "second line must be a separator: {}",
        lines[1]
    );
}
