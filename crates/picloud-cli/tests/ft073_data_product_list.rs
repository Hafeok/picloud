//! FT-073 — picloud data-product list and picloud data-domain list
//!
//! Covers TC-277 (scenario) and TC-334 (exit-criteria).
//!
//! TC-277: picloud data-product list and data-domain list return expected entries.
//!   Verifies that SPARQL queries are correctly constructed, response parsing
//!   extracts all fields, IRI segments are resolved to short names, and the
//!   table output includes all required columns.
//!
//! TC-334: Data CLI exit — data-product list and data-domain list work.
//!   Exit-criteria test validating that both commands handle empty results,
//!   multiple entries with mixed states, nested JSON response formats,
//!   IRI extraction edge cases, and table formatting correctly.

use picloud_cli::commands;
use picloud_cli::commands::{DataDomainListRow, DataProductListRow};
use serde_json::json;

// ===========================================================================
// TC-277 — picloud data-product list and data-domain list return expected entries
// ===========================================================================

/// The data-domain list SPARQL query fetches name, steward, and sensitivity.
/// The data-product list SPARQL query fetches name, product, domain, version,
/// and status. Parsing extracts all fields correctly, IRI paths are resolved
/// to short names, and table output includes all required columns.
#[test]
fn tc277_picloud_data_product_list_and_data_domain_list_return_expected_entries() {
    // -----------------------------------------------------------------------
    // 1. Data-domain SPARQL query construction
    // -----------------------------------------------------------------------

    let sparql = commands::data_domain_list_sparql();

    // Must query for DataDomain type
    assert!(
        sparql.contains("picloud:DataDomain")
            || sparql.contains("DataDomain"),
        "SPARQL must select DataDomain type: {}",
        sparql
    );

    // Must select name, steward, sensitivity
    assert!(
        sparql.contains("picloud:name ?name"),
        "SPARQL must select name: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:steward ?steward"),
        "SPARQL must select steward: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:sensitivity ?sensitivity"),
        "SPARQL must select sensitivity: {}",
        sparql
    );

    // Must order results
    assert!(
        sparql.contains("ORDER BY"),
        "SPARQL must include ORDER BY: {}",
        sparql
    );

    // -----------------------------------------------------------------------
    // 2. Data-domain response parsing
    // -----------------------------------------------------------------------

    let body = json!({
        "bindings": [
            {
                "domain": { "value": "https://picloud.local/data-domains/geospatial" },
                "name": { "value": "geospatial" },
                "steward": { "value": "https://picloud.local/identity/alice" },
                "sensitivity": { "value": "internal" }
            }
        ]
    });

    let rows = commands::parse_data_domain_list(&body);
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.name, "geospatial");
    assert_eq!(row.steward, "alice", "steward IRI must be resolved to short name");
    assert_eq!(row.sensitivity, "internal");

    // -----------------------------------------------------------------------
    // 3. Data-domain table formatting
    // -----------------------------------------------------------------------

    let table = commands::format_data_domain_table(&rows);

    // Header columns
    assert!(table.contains("NAME"), "table must have NAME column");
    assert!(table.contains("STEWARD"), "table must have STEWARD column");
    assert!(
        table.contains("SENSITIVITY"),
        "table must have SENSITIVITY column"
    );

    // Row data
    assert!(table.contains("geospatial"), "table must show domain name");
    assert!(table.contains("alice"), "table must show steward name");
    assert!(table.contains("internal"), "table must show sensitivity");

    // -----------------------------------------------------------------------
    // 4. Data-product SPARQL query construction
    // -----------------------------------------------------------------------

    let sparql = commands::data_product_list_sparql();

    // Must query for DataProduct type
    assert!(
        sparql.contains("picloud:DataProduct")
            || sparql.contains("DataProduct"),
        "SPARQL must select DataProduct type: {}",
        sparql
    );

    // Must select name, product, domain, version, status
    assert!(
        sparql.contains("picloud:name ?name"),
        "SPARQL must select name: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:product ?product"),
        "SPARQL must select product: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:domain ?domain"),
        "SPARQL must select domain: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:version ?version"),
        "SPARQL must select version: {}",
        sparql
    );
    assert!(
        sparql.contains("picloud:status ?status"),
        "SPARQL must select status: {}",
        sparql
    );

    // Must order results
    assert!(
        sparql.contains("ORDER BY"),
        "SPARQL must include ORDER BY: {}",
        sparql
    );

    // -----------------------------------------------------------------------
    // 5. Data-product response parsing
    // -----------------------------------------------------------------------

    let body = json!({
        "bindings": [
            {
                "dp": { "value": "https://picloud.local/products/photo-app/data-products/photo-locations" },
                "name": { "value": "photo-locations" },
                "product": { "value": "https://picloud.local/products/photo-app" },
                "domain": { "value": "https://picloud.local/data-domains/geospatial" },
                "version": { "value": "1.0.0" },
                "status": { "value": "ready" }
            }
        ]
    });

    let rows = commands::parse_data_product_list(&body);
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row.name, "photo-locations");
    assert_eq!(row.product, "photo-app", "product IRI must be resolved to short name");
    assert_eq!(row.domain, "geospatial", "domain IRI must be resolved to short name");
    assert_eq!(row.version, "1.0.0");
    assert_eq!(row.status, "ready");

    // -----------------------------------------------------------------------
    // 6. Data-product table formatting
    // -----------------------------------------------------------------------

    let table = commands::format_data_product_table(&rows);

    // Header columns
    assert!(table.contains("NAME"), "table must have NAME column");
    assert!(table.contains("PRODUCT"), "table must have PRODUCT column");
    assert!(table.contains("DOMAIN"), "table must have DOMAIN column");
    assert!(table.contains("VERSION"), "table must have VERSION column");
    assert!(table.contains("STATUS"), "table must have STATUS column");

    // Row data
    assert!(
        table.contains("photo-locations"),
        "table must show data product name"
    );
    assert!(
        table.contains("photo-app"),
        "table must show product name"
    );
    assert!(
        table.contains("geospatial"),
        "table must show domain name"
    );
    assert!(table.contains("1.0.0"), "table must show version");
    assert!(table.contains("ready"), "table must show status");
}

// ===========================================================================
// TC-334 — Data CLI exit — data-product list and data-domain list work
// ===========================================================================

/// Exit criteria: both data-domain list and data-product list handle empty
/// results, multiple entries, nested JSON, IRI extraction edge cases, missing
/// body structures, and table formatting correctly.
#[test]
fn tc334_data_cli_exit_data_product_list_and_data_domain_list_work() {
    // -----------------------------------------------------------------------
    // A. Empty result sets
    // -----------------------------------------------------------------------

    let empty_body = json!({ "bindings": [] });

    let domain_rows = commands::parse_data_domain_list(&empty_body);
    assert!(domain_rows.is_empty(), "empty bindings must produce no domain rows");
    let table = commands::format_data_domain_table(&domain_rows);
    assert_eq!(
        table, "No data domains declared.",
        "empty domain table must show placeholder message"
    );

    let product_rows = commands::parse_data_product_list(&empty_body);
    assert!(product_rows.is_empty(), "empty bindings must produce no product rows");
    let table = commands::format_data_product_table(&product_rows);
    assert_eq!(
        table, "No data products declared.",
        "empty product table must show placeholder message"
    );

    // -----------------------------------------------------------------------
    // B. Multiple data domains with different sensitivities
    // -----------------------------------------------------------------------

    let body = json!({
        "bindings": [
            {
                "domain": { "value": "https://picloud.local/data-domains/geospatial" },
                "name": { "value": "geospatial" },
                "steward": { "value": "https://picloud.local/identity/alice" },
                "sensitivity": { "value": "internal" }
            },
            {
                "domain": { "value": "https://picloud.local/data-domains/engineering" },
                "name": { "value": "engineering" },
                "steward": { "value": "https://picloud.local/identity/bob" },
                "sensitivity": { "value": "public" }
            },
            {
                "domain": { "value": "https://picloud.local/data-domains/pii" },
                "name": { "value": "pii" },
                "steward": { "value": "https://picloud.local/identity/carol" },
                "sensitivity": { "value": "restricted" }
            }
        ]
    });

    let rows = commands::parse_data_domain_list(&body);
    assert_eq!(rows.len(), 3, "must parse all three data domains");

    assert_eq!(rows[0].name, "geospatial");
    assert_eq!(rows[0].steward, "alice");
    assert_eq!(rows[0].sensitivity, "internal");

    assert_eq!(rows[1].name, "engineering");
    assert_eq!(rows[1].steward, "bob");
    assert_eq!(rows[1].sensitivity, "public");

    assert_eq!(rows[2].name, "pii");
    assert_eq!(rows[2].steward, "carol");
    assert_eq!(rows[2].sensitivity, "restricted");

    let table = commands::format_data_domain_table(&rows);
    assert!(table.contains("geospatial"), "must list geospatial");
    assert!(table.contains("engineering"), "must list engineering");
    assert!(table.contains("pii"), "must list pii");
    assert!(table.contains("internal"), "must show internal sensitivity");
    assert!(table.contains("public"), "must show public sensitivity");
    assert!(table.contains("restricted"), "must show restricted sensitivity");

    // -----------------------------------------------------------------------
    // C. Multiple data products with mixed statuses
    // -----------------------------------------------------------------------

    let body = json!({
        "bindings": [
            {
                "dp": { "value": "https://picloud.local/products/photo-app/data-products/photo-locations" },
                "name": { "value": "photo-locations" },
                "product": { "value": "https://picloud.local/products/photo-app" },
                "domain": { "value": "https://picloud.local/data-domains/geospatial" },
                "version": { "value": "1.0.0" },
                "status": { "value": "ready" }
            },
            {
                "dp": { "value": "https://picloud.local/products/maps-app/data-products/places-index" },
                "name": { "value": "places-index" },
                "product": { "value": "https://picloud.local/products/maps-app" },
                "domain": { "value": "https://picloud.local/data-domains/geospatial" },
                "version": { "value": "2.1.0" },
                "status": { "value": "declared" }
            },
            {
                "dp": { "value": "https://picloud.local/products/analytics-svc/data-products/user-metrics" },
                "name": { "value": "user-metrics" },
                "product": { "value": "https://picloud.local/products/analytics-svc" },
                "domain": { "value": "https://picloud.local/data-domains/pii" },
                "version": { "value": "0.9.0" },
                "status": { "value": "provisioning" }
            }
        ]
    });

    let rows = commands::parse_data_product_list(&body);
    assert_eq!(rows.len(), 3, "must parse all three data products");

    assert_eq!(rows[0].name, "photo-locations");
    assert_eq!(rows[0].product, "photo-app");
    assert_eq!(rows[0].domain, "geospatial");
    assert_eq!(rows[0].version, "1.0.0");
    assert_eq!(rows[0].status, "ready");

    assert_eq!(rows[1].name, "places-index");
    assert_eq!(rows[1].product, "maps-app");
    assert_eq!(rows[1].domain, "geospatial");
    assert_eq!(rows[1].version, "2.1.0");
    assert_eq!(rows[1].status, "declared");

    assert_eq!(rows[2].name, "user-metrics");
    assert_eq!(rows[2].product, "analytics-svc");
    assert_eq!(rows[2].domain, "pii");
    assert_eq!(rows[2].version, "0.9.0");
    assert_eq!(rows[2].status, "provisioning");

    let table = commands::format_data_product_table(&rows);
    assert!(table.contains("photo-locations"), "must list photo-locations");
    assert!(table.contains("places-index"), "must list places-index");
    assert!(table.contains("user-metrics"), "must list user-metrics");
    assert!(
        table.contains("ready") && table.contains("declared") && table.contains("provisioning"),
        "must show all status values"
    );

    // -----------------------------------------------------------------------
    // D. Nested results format (results -> bindings)
    // -----------------------------------------------------------------------

    let nested_domain = json!({
        "results": {
            "bindings": [
                {
                    "domain": { "value": "https://picloud.local/data-domains/finance" },
                    "name": { "value": "finance" },
                    "steward": { "value": "https://picloud.local/identity/dave" },
                    "sensitivity": { "value": "confidential" }
                }
            ]
        }
    });

    let rows = commands::parse_data_domain_list(&nested_domain);
    assert_eq!(rows.len(), 1, "must parse from nested results.bindings");
    assert_eq!(rows[0].name, "finance");
    assert_eq!(rows[0].steward, "dave");
    assert_eq!(rows[0].sensitivity, "confidential");

    let nested_product = json!({
        "results": {
            "bindings": [
                {
                    "dp": { "value": "https://picloud.local/products/billing-svc/data-products/invoices" },
                    "name": { "value": "invoices" },
                    "product": { "value": "https://picloud.local/products/billing-svc" },
                    "domain": { "value": "https://picloud.local/data-domains/finance" },
                    "version": { "value": "3.0.0" },
                    "status": { "value": "ready" }
                }
            ]
        }
    });

    let rows = commands::parse_data_product_list(&nested_product);
    assert_eq!(rows.len(), 1, "must parse from nested results.bindings");
    assert_eq!(rows[0].name, "invoices");
    assert_eq!(rows[0].product, "billing-svc");
    assert_eq!(rows[0].domain, "finance");

    // -----------------------------------------------------------------------
    // E. Missing body structure returns empty
    // -----------------------------------------------------------------------

    let bad_body = json!({ "unexpected": "format" });
    let rows = commands::parse_data_domain_list(&bad_body);
    assert!(rows.is_empty(), "non-matching body must return empty domain list");

    let rows = commands::parse_data_product_list(&bad_body);
    assert!(rows.is_empty(), "non-matching body must return empty product list");

    let null_body = json!(null);
    let rows = commands::parse_data_domain_list(&null_body);
    assert!(rows.is_empty(), "null body must return empty domain list");

    let rows = commands::parse_data_product_list(&null_body);
    assert!(rows.is_empty(), "null body must return empty product list");

    // -----------------------------------------------------------------------
    // F. IRI extraction — steward, product, and domain from various formats
    // -----------------------------------------------------------------------

    // Steward IRI → short name
    let body = json!({
        "bindings": [{
            "domain": { "value": "https://picloud.local/data-domains/test" },
            "name": { "value": "test-domain" },
            "steward": { "value": "https://picloud.local/identity/long-name-steward" },
            "sensitivity": { "value": "public" }
        }]
    });
    let rows = commands::parse_data_domain_list(&body);
    assert_eq!(rows[0].steward, "long-name-steward", "steward name must be extracted from IRI");

    // Plain name (no IRI structure) passes through
    let body_plain = json!({
        "bindings": [{
            "domain": { "value": "test" },
            "name": { "value": "plain-domain" },
            "steward": { "value": "just-a-name" },
            "sensitivity": { "value": "internal" }
        }]
    });
    let rows = commands::parse_data_domain_list(&body_plain);
    assert_eq!(
        rows[0].steward, "just-a-name",
        "plain names must pass through unchanged"
    );

    // Product and domain IRI extraction for data products
    let body = json!({
        "bindings": [{
            "dp": { "value": "https://picloud.local/products/my-svc/data-products/dp1" },
            "name": { "value": "dp1" },
            "product": { "value": "https://picloud.local/products/my-svc" },
            "domain": { "value": "https://picloud.local/data-domains/my-domain" },
            "version": { "value": "1.0.0" },
            "status": { "value": "ready" }
        }]
    });
    let rows = commands::parse_data_product_list(&body);
    assert_eq!(rows[0].product, "my-svc", "product name must be extracted from IRI");
    assert_eq!(rows[0].domain, "my-domain", "domain name must be extracted from IRI");

    // Plain names for product and domain pass through
    let body_plain = json!({
        "bindings": [{
            "dp": { "value": "dp-plain" },
            "name": { "value": "dp-plain" },
            "product": { "value": "just-product" },
            "domain": { "value": "just-domain" },
            "version": { "value": "0.1.0" },
            "status": { "value": "declared" }
        }]
    });
    let rows = commands::parse_data_product_list(&body_plain);
    assert_eq!(rows[0].product, "just-product", "plain product must pass through");
    assert_eq!(rows[0].domain, "just-domain", "plain domain must pass through");

    // -----------------------------------------------------------------------
    // G. SPARQL queries are well-formed and URL-encodable
    // -----------------------------------------------------------------------

    let domain_sparql = commands::data_domain_list_sparql();
    let encoded = commands::urlencoding(domain_sparql);
    assert!(
        !encoded.contains(' '),
        "encoded domain SPARQL must not contain literal spaces"
    );
    assert!(
        encoded.contains("%7B") && encoded.contains("%7D"),
        "braces must be encoded in domain SPARQL"
    );

    let product_sparql = commands::data_product_list_sparql();
    let encoded = commands::urlencoding(product_sparql);
    assert!(
        !encoded.contains(' '),
        "encoded product SPARQL must not contain literal spaces"
    );
    assert!(
        encoded.contains("%7B") && encoded.contains("%7D"),
        "braces must be encoded in product SPARQL"
    );

    // -----------------------------------------------------------------------
    // H. Table separator lines
    // -----------------------------------------------------------------------

    let single_domain = vec![DataDomainListRow {
        name: "x".to_string(),
        steward: "y".to_string(),
        sensitivity: "public".to_string(),
    }];
    let table = commands::format_data_domain_table(&single_domain);
    let lines: Vec<&str> = table.lines().collect();
    assert!(lines.len() >= 3, "domain table must have header, separator, and data rows");
    assert!(
        lines[1].chars().all(|c| c == '-'),
        "second line must be a separator: {}",
        lines[1]
    );

    let single_product = vec![DataProductListRow {
        name: "x".to_string(),
        product: "p".to_string(),
        domain: "d".to_string(),
        version: "1.0.0".to_string(),
        status: "ready".to_string(),
    }];
    let table = commands::format_data_product_table(&single_product);
    let lines: Vec<&str> = table.lines().collect();
    assert!(lines.len() >= 3, "product table must have header, separator, and data rows");
    assert!(
        lines[1].chars().all(|c| c == '-'),
        "second line must be a separator: {}",
        lines[1]
    );
}
