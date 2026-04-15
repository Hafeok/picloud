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
