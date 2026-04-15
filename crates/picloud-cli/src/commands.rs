//! Pure-function helpers for CLI command URL building, SSE parsing,
//! and response formatting — extracted so they can be unit-tested without
//! a running cluster.

use serde_json::Value;

// ---------------------------------------------------------------------------
// URL encoding
// ---------------------------------------------------------------------------

/// Simple URL encoding for query parameters.
pub fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('{', "%7B")
        .replace('}', "%7D")
}

// ---------------------------------------------------------------------------
// Events stream
// ---------------------------------------------------------------------------

/// Build the SSE endpoint path for `picloud events stream`.
///
/// - With `--product photo-app`:  `/products/photo-app/events`
/// - With `--product photo-app --correlation_id abc`:
///       `/products/photo-app/events?correlation_id=abc`
/// - Without product: `/api/events/stream`
/// - Without product, with correlation_id: `/api/events/stream?correlation_id=abc`
pub fn events_stream_path(
    product: Option<&str>,
    correlation_id: Option<&str>,
) -> String {
    if let Some(p) = product {
        let mut url = format!("/products/{}/events", p);
        if let Some(c) = correlation_id {
            url = format!("{}?correlation_id={}", url, c);
        }
        url
    } else {
        let mut url = "/api/events/stream".to_string();
        let mut params = vec![];
        if let Some(c) = correlation_id {
            params.push(format!("correlation_id={}", c));
        }
        if !params.is_empty() {
            url = format!("{}?{}", url, params.join("&"));
        }
        url
    }
}

// ---------------------------------------------------------------------------
// Graph query
// ---------------------------------------------------------------------------

/// Build the endpoint path for `picloud graph query`.
///
/// - With `--product photo-app`: `/products/photo-app/graph?query={sparql}`
/// - Without product: `/graph?query={sparql}`
pub fn graph_query_path(sparql: &str, product: Option<&str>) -> String {
    if let Some(p) = product {
        format!(
            "/products/{}/graph?query={}",
            p,
            urlencoding(sparql)
        )
    } else {
        format!("/graph?query={}", urlencoding(sparql))
    }
}

// ---------------------------------------------------------------------------
// Telemetry query
// ---------------------------------------------------------------------------

/// Build the endpoint path for `picloud telemetry query` (legacy filter mode).
///
/// Returns `None` if the signal type is unknown.
pub fn telemetry_query_path(
    signal: &str,
    from: Option<&str>,
    to: Option<&str>,
    service: Option<&str>,
) -> Option<String> {
    let endpoint = match signal {
        "traces" | "spans" => "/telemetry/spans",
        "metrics" => "/telemetry/metrics",
        _ => return None,
    };

    let mut params = Vec::new();
    if let Some(f) = from {
        params.push(format!("from={}", urlencoding(f)));
    }
    if let Some(t) = to {
        params.push(format!("to={}", urlencoding(t)));
    }
    if let Some(s) = service {
        params.push(format!("service={}", urlencoding(s)));
    }

    if params.is_empty() {
        Some(endpoint.to_string())
    } else {
        Some(format!("{}?{}", endpoint, params.join("&")))
    }
}

/// Build the JSON body for `picloud telemetry query --sql`.
pub fn telemetry_sql_body(signal: &str, sql: &str) -> Value {
    serde_json::json!({
        "signal": signal,
        "sql": sql,
    })
}

// ---------------------------------------------------------------------------
// Identity token — device flow helpers
// ---------------------------------------------------------------------------

/// Parse the device flow begin response.
///
/// Returns `(device_code, verification_url, interval_secs, expires_in_secs)`.
pub fn parse_device_flow_begin(resp: &Value) -> (String, String, u64, u64) {
    let device_code = resp["device_code"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let verification_url = resp["verification_url"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let interval = resp["interval_secs"].as_u64().unwrap_or(5);
    let expires_in = resp["expires_in_secs"].as_u64().unwrap_or(600);
    (device_code, verification_url, interval, expires_in)
}

/// Parse a device flow poll response.
///
/// Returns the status string ("complete", "pending", "expired", or raw value).
pub fn parse_device_flow_poll_status(resp: &Value) -> &str {
    resp["status"].as_str().unwrap_or("unknown")
}

/// Extract the access token from a completed device flow poll response.
pub fn extract_access_token(resp: &Value) -> &str {
    resp["access_token"].as_str().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// SSE event parsing
// ---------------------------------------------------------------------------

/// A parsed SSE event.
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    /// A `data: {json}` line with the parsed JSON value.
    Data(Value),
    /// A `data: {raw_text}` line that was not valid JSON.
    RawData(String),
    /// An `event: name` line.
    EventType(String),
    /// An empty line, comment, or otherwise ignorable line.
    Ignored,
}

/// Parse a single SSE text line into a structured event.
pub fn parse_sse_line(line: &str) -> SseEvent {
    let line = line.trim_end_matches('\r');
    if line.starts_with("data: ") {
        let data = &line[6..];
        match serde_json::from_str::<Value>(data) {
            Ok(json) => SseEvent::Data(json),
            Err(_) => SseEvent::RawData(data.to_string()),
        }
    } else if line.starts_with("event: ") {
        SseEvent::EventType(line[7..].to_string())
    } else {
        SseEvent::Ignored
    }
}

/// Format an SSE JSON event into human-readable output matching what the CLI prints.
pub fn format_sse_event(json: &Value) -> String {
    let event_type = json
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let source = json.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let timestamp = json
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut output = format!("[{}] {} — {}", timestamp, event_type, source);
    if let Some(payload) = json.get("payload") {
        let pretty = serde_json::to_string_pretty(payload)
            .unwrap_or_default()
            .replace('\n', "\n  ");
        output.push_str(&format!("\n  {}", pretty));
    }
    output
}

/// Format a graph query JSON response for display.
pub fn format_graph_results(body: &Value) -> String {
    serde_json::to_string_pretty(body).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Parse SQL WHERE clause
// ---------------------------------------------------------------------------

/// Parse a simple SQL WHERE clause into field-value pairs.
///
/// Supports patterns like:
///   `SELECT * FROM traces WHERE product = 'photo-app' AND duration_ms > 100`
///
/// Extracts `field = 'value'` and `field > number` conditions.
pub fn parse_sql_where_clause(sql: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();

    let sql_upper = sql.to_uppercase();
    let where_pos = match sql_upper.find("WHERE") {
        Some(pos) => pos + 5,
        None => return results,
    };

    let where_clause = &sql[where_pos..];

    let mut parts = Vec::new();
    let mut remaining = where_clause.trim();
    loop {
        let upper = remaining.to_uppercase();
        if let Some(pos) = upper.find(" AND ") {
            parts.push(remaining[..pos].trim());
            remaining = remaining[pos + 5..].trim();
        } else {
            if !remaining.is_empty() {
                parts.push(remaining.trim());
            }
            break;
        }
    }

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (field, value) = if let Some(pos) = part.find(">=") {
            let field = part[..pos].trim();
            let val = part[pos + 2..].trim().trim_matches('\'').trim_matches('"');
            (field, val)
        } else if let Some(pos) = part.find('>') {
            let field = part[..pos].trim();
            let val = part[pos + 1..].trim().trim_matches('\'').trim_matches('"');
            (field, val)
        } else if let Some(pos) = part.find('=') {
            let field = part[..pos].trim();
            let val = part[pos + 1..].trim().trim_matches('\'').trim_matches('"');
            (field, val)
        } else {
            continue;
        };

        if !field.is_empty() && !value.is_empty() {
            results.push((field.to_lowercase(), value.to_string()));
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Capability list (FT-064)
// ---------------------------------------------------------------------------

/// A single row in the capability list output.
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityListRow {
    pub name: String,
    pub version: String,
    pub fulfilled: bool,
    pub implementors: Vec<String>,
    pub consumers: Vec<String>,
}

/// Build the SPARQL query for listing all capabilities with implementors,
/// consumers, and fulfilment status.
///
/// Uses GROUP_CONCAT to aggregate implementors and consumers into
/// comma-separated strings, keeping the result set to one row per capability.
pub fn capability_list_sparql() -> &'static str {
    "PREFIX picloud: <https://picloud.local/ontology#> \
     PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> \
     SELECT ?name ?version ?status \
     (GROUP_CONCAT(DISTINCT ?implementor; SEPARATOR=\", \") AS ?implementors) \
     (GROUP_CONCAT(DISTINCT ?consumer; SEPARATOR=\", \") AS ?consumers) \
     WHERE { \
     ?cap rdf:type picloud:Capability . \
     ?cap picloud:name ?name . \
     ?cap picloud:version ?version . \
     OPTIONAL { ?cap picloud:status ?status } \
     OPTIONAL { ?cap picloud:implementedBy ?implementor } \
     OPTIONAL { ?cap picloud:consumedBy ?consumer } \
     } GROUP BY ?cap ?name ?version ?status ORDER BY ?name"
}

/// Determine whether a capability is fulfilled.
///
/// A capability is fulfilled when it has at least one implementor.
pub fn capability_is_fulfilled(implementors: &[String]) -> bool {
    !implementors.is_empty()
}

/// Extract a plain string value from a SPARQL binding field.
///
/// SPARQL JSON results encode values as either `"field": {"value": "..."}` or
/// sometimes as a plain string. This helper handles both.
fn binding_str<'a>(row: &'a Value, field: &str) -> &'a str {
    row.get(field)
        .and_then(|v| v.get("value").and_then(|v| v.as_str()).or_else(|| v.as_str()))
        .unwrap_or("")
}

/// Parse the JSON response body from a SPARQL capability list query into
/// structured rows.
///
/// Expects the standard SPARQL JSON results format with a top-level
/// `"bindings"` array (possibly nested under `"results"`).
pub fn parse_capability_list(body: &Value) -> Vec<CapabilityListRow> {
    let bindings = body
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array())
        .or_else(|| body.get("bindings").and_then(|b| b.as_array()));

    let bindings = match bindings {
        Some(b) => b,
        None => return Vec::new(),
    };

    bindings
        .iter()
        .map(|row| {
            let name = binding_str(row, "name").to_string();
            let version = binding_str(row, "version").to_string();
            let implementors_raw = binding_str(row, "implementors");
            let consumers_raw = binding_str(row, "consumers");

            let implementors: Vec<String> = if implementors_raw.is_empty() {
                Vec::new()
            } else {
                implementors_raw
                    .split(", ")
                    .map(|s| extract_product_name(s).to_string())
                    .collect()
            };

            let consumers: Vec<String> = if consumers_raw.is_empty() {
                Vec::new()
            } else {
                consumers_raw
                    .split(", ")
                    .map(|s| extract_product_name(s).to_string())
                    .collect()
            };

            let fulfilled = capability_is_fulfilled(&implementors);

            CapabilityListRow {
                name,
                version,
                fulfilled,
                implementors,
                consumers,
            }
        })
        .collect()
}

/// Extract the product name from a product IRI.
///
/// Given `https://picloud.local/products/photo-app`, returns `photo-app`.
/// If the IRI doesn't match the expected pattern, returns the full string.
fn extract_product_name(iri: &str) -> &str {
    iri.rsplit("/products/")
        .next()
        .and_then(|s| s.split('/').next())
        .unwrap_or(iri)
}

/// Extract the last segment of an IRI path.
///
/// Given `https://picloud.local/data-domains/geospatial`, returns `geospatial`.
/// Given `https://picloud.local/identity/alice`, returns `alice`.
/// If the IRI doesn't contain `/`, returns the full string.
fn extract_last_segment(iri: &str) -> &str {
    iri.rsplit('/').next().unwrap_or(iri)
}

/// Format capability list rows into a human-readable table.
///
/// Columns: NAME, VERSION, FULFILLED, IMPLEMENTORS, CONSUMERS
pub fn format_capability_table(rows: &[CapabilityListRow]) -> String {
    if rows.is_empty() {
        return "No capabilities declared.".to_string();
    }

    let mut output = format!(
        "{:<25} {:<10} {:<12} {:<30} {:<30}",
        "NAME", "VERSION", "FULFILLED", "IMPLEMENTORS", "CONSUMERS"
    );
    output.push('\n');
    output.push_str(&"-".repeat(107));

    for row in rows {
        let fulfilled_str = if row.fulfilled { "yes" } else { "no" };
        let implementors_str = if row.implementors.is_empty() {
            "-".to_string()
        } else {
            row.implementors.join(", ")
        };
        let consumers_str = if row.consumers.is_empty() {
            "-".to_string()
        } else {
            row.consumers.join(", ")
        };

        output.push('\n');
        output.push_str(&format!(
            "{:<25} {:<10} {:<12} {:<30} {:<30}",
            row.name, row.version, fulfilled_str, implementors_str, consumers_str,
        ));
    }

    output
}

// ---------------------------------------------------------------------------
// Data-domain list (FT-073)
// ---------------------------------------------------------------------------

/// A single row in the data-domain list output.
#[derive(Debug, Clone, PartialEq)]
pub struct DataDomainListRow {
    pub name: String,
    pub steward: String,
    pub sensitivity: String,
}

/// Build the SPARQL query for listing all data domains with steward and
/// sensitivity level.
pub fn data_domain_list_sparql() -> &'static str {
    "PREFIX picloud: <https://picloud.local/ontology#> \
     PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> \
     SELECT ?domain ?name ?steward ?sensitivity \
     WHERE { \
     ?domain rdf:type picloud:DataDomain . \
     ?domain picloud:name ?name . \
     ?domain picloud:steward ?steward . \
     ?domain picloud:sensitivity ?sensitivity . \
     } ORDER BY ?name"
}

/// Parse the JSON response body from a SPARQL data-domain list query into
/// structured rows.
///
/// Expects the standard SPARQL JSON results format with a top-level
/// `"bindings"` array (possibly nested under `"results"`).
pub fn parse_data_domain_list(body: &Value) -> Vec<DataDomainListRow> {
    let bindings = body
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array())
        .or_else(|| body.get("bindings").and_then(|b| b.as_array()));

    let bindings = match bindings {
        Some(b) => b,
        None => return Vec::new(),
    };

    bindings
        .iter()
        .map(|row| {
            let name = binding_str(row, "name").to_string();
            let steward_raw = binding_str(row, "steward");
            let steward = extract_last_segment(steward_raw).to_string();
            let sensitivity = binding_str(row, "sensitivity").to_string();

            DataDomainListRow {
                name,
                steward,
                sensitivity,
            }
        })
        .collect()
}

/// Format data-domain list rows into a human-readable table.
///
/// Columns: NAME, STEWARD, SENSITIVITY
pub fn format_data_domain_table(rows: &[DataDomainListRow]) -> String {
    if rows.is_empty() {
        return "No data domains declared.".to_string();
    }

    let mut output = format!(
        "{:<25} {:<25} {:<15}",
        "NAME", "STEWARD", "SENSITIVITY"
    );
    output.push('\n');
    output.push_str(&"-".repeat(65));

    for row in rows {
        output.push('\n');
        output.push_str(&format!(
            "{:<25} {:<25} {:<15}",
            row.name, row.steward, row.sensitivity,
        ));
    }

    output
}

// ---------------------------------------------------------------------------
// Data-product list (FT-073)
// ---------------------------------------------------------------------------

/// A single row in the data-product list output.
#[derive(Debug, Clone, PartialEq)]
pub struct DataProductListRow {
    pub name: String,
    pub product: String,
    pub domain: String,
    pub version: String,
    pub status: String,
}

/// Build the SPARQL query for listing all data products with product, domain,
/// version, and lifecycle status.
pub fn data_product_list_sparql() -> &'static str {
    "PREFIX picloud: <https://picloud.local/ontology#> \
     PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> \
     SELECT ?dp ?name ?product ?domain ?version ?status \
     WHERE { \
     ?dp rdf:type picloud:DataProduct . \
     ?dp picloud:name ?name . \
     ?dp picloud:product ?product . \
     ?dp picloud:domain ?domain . \
     ?dp picloud:version ?version . \
     ?dp picloud:status ?status . \
     } ORDER BY ?product ?name"
}

/// Parse the JSON response body from a SPARQL data-product list query into
/// structured rows.
///
/// Expects the standard SPARQL JSON results format with a top-level
/// `"bindings"` array (possibly nested under `"results"`).
pub fn parse_data_product_list(body: &Value) -> Vec<DataProductListRow> {
    let bindings = body
        .get("results")
        .and_then(|r| r.get("bindings"))
        .and_then(|b| b.as_array())
        .or_else(|| body.get("bindings").and_then(|b| b.as_array()));

    let bindings = match bindings {
        Some(b) => b,
        None => return Vec::new(),
    };

    bindings
        .iter()
        .map(|row| {
            let name = binding_str(row, "name").to_string();
            let product_raw = binding_str(row, "product");
            let product = extract_product_name(product_raw).to_string();
            let domain_raw = binding_str(row, "domain");
            let domain = extract_last_segment(domain_raw).to_string();
            let version = binding_str(row, "version").to_string();
            let status = binding_str(row, "status").to_string();

            DataProductListRow {
                name,
                product,
                domain,
                version,
                status,
            }
        })
        .collect()
}

/// Format data-product list rows into a human-readable table.
///
/// Columns: NAME, PRODUCT, DOMAIN, VERSION, STATUS
pub fn format_data_product_table(rows: &[DataProductListRow]) -> String {
    if rows.is_empty() {
        return "No data products declared.".to_string();
    }

    let mut output = format!(
        "{:<25} {:<20} {:<20} {:<10} {:<12}",
        "NAME", "PRODUCT", "DOMAIN", "VERSION", "STATUS"
    );
    output.push('\n');
    output.push_str(&"-".repeat(87));

    for row in rows {
        output.push('\n');
        output.push_str(&format!(
            "{:<25} {:<20} {:<20} {:<10} {:<12}",
            row.name, row.product, row.domain, row.version, row.status,
        ));
    }

    output
}
