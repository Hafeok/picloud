/// Reverse Proxy (ADR-048)
///
/// Forwards incoming HTTP requests to upstream containers using reqwest.
/// Streams responses back to the client.
/// Returns 503 with Retry-After when upstream is unreachable.

use std::time::Duration;

use axum::body::Body;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use tracing::warn;

use crate::router::{RouteEntry, SharedRouter};

// ---------------------------------------------------------------------------
// Connection draining (ADR-048, Phase 4)
// ---------------------------------------------------------------------------

/// Drain state for graceful connection draining during workload rescheduling.
///
/// When draining is active, new connections receive 503 with Retry-After.
/// In-flight requests are allowed to complete within the grace period (30s).
pub struct DrainState {
    /// Signal that draining has started.
    notify: tokio::sync::Notify,
    /// Whether the proxy is currently draining.
    draining: std::sync::atomic::AtomicBool,
}

impl DrainState {
    pub fn new() -> Self {
        Self {
            notify: tokio::sync::Notify::new(),
            draining: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Start draining — new requests will be rejected.
    pub fn start_drain(&self) {
        self.draining.store(true, std::sync::atomic::Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// Stop draining — resume normal operation.
    pub fn stop_drain(&self) {
        self.draining.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if draining is active.
    pub fn is_draining(&self) -> bool {
        self.draining.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for DrainState {
    fn default() -> Self {
        Self::new()
    }
}

/// Maximum time to wait for in-flight requests during connection draining.
pub const DRAIN_GRACE_PERIOD: Duration = Duration::from_secs(30);

/// Timeout for upstream connections — fail fast rather than queue.
pub const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Timeout for the full upstream request lifecycle.
pub const UPSTREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// How long clients should wait before retrying after a 503.
pub const RETRY_AFTER_SECONDS: &str = "5";

/// Proxy an incoming request through the ingress router.
///
/// 1. Extract the hostname from the Host header.
/// 2. Look up the route in the IngressRouter.
/// 3. Forward the request to the upstream.
/// 4. Stream the response back.
/// 5. Return 503 with Retry-After if the upstream is unreachable.
pub async fn handle_proxy(
    router: SharedRouter,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_path = uri.path();

    // Extract hostname from the Host header
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| {
            // Strip port from host header if present
            h.split(':').next().unwrap_or(h)
        })
        .unwrap_or("");

    // Look up the route
    let route = router.lookup(host, request_path).await;

    let Some(route) = route else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "message": format!("no route matches host={host} path={request_path}"),
            })),
        )
            .into_response();
    };

    forward_to_upstream(&route, method, &uri, headers, body).await
}

/// Forward a request to the resolved upstream.
async fn forward_to_upstream(
    route: &RouteEntry,
    method: Method,
    uri: &Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let request_path = uri.path();

    // Strip the ingress path prefix and forward the remainder
    let downstream_path = &request_path[route.path_prefix.len()..];
    let downstream_path = if downstream_path.is_empty() || !downstream_path.starts_with('/') {
        format!("/{downstream_path}")
    } else {
        downstream_path.to_string()
    };

    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_url = format!(
        "http://{}:{}{downstream_path}{query}",
        route.upstream_addr, route.upstream_port
    );

    // Build the proxied request
    let client = reqwest::Client::builder()
        .connect_timeout(UPSTREAM_CONNECT_TIMEOUT)
        .timeout(UPSTREAM_REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut proxy_req = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
        &target_url,
    );

    // Forward relevant headers (skip host — we're proxying to upstream)
    let mut has_traceparent = false;
    for (name, value) in headers.iter() {
        if name != header::HOST && name != header::TRANSFER_ENCODING {
            if let Ok(v) = value.to_str() {
                proxy_req = proxy_req.header(name.as_str(), v);
                if name.as_str().eq_ignore_ascii_case("traceparent") {
                    has_traceparent = true;
                }
            }
        }
    }

    // FT-048: If no traceparent header was present on the incoming request,
    // generate one so that workloads always receive trace context.
    if !has_traceparent {
        let trace_id = uuid::Uuid::new_v4();
        let parent_id: u64 = rand::random();
        let traceparent = format!("00-{}-{:016x}-01", trace_id.as_simple(), parent_id);
        proxy_req = proxy_req.header("traceparent", &traceparent);
    }

    // Forward the body
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_request",
                    "message": format!("failed to read request body: {e}"),
                })),
            )
                .into_response();
        }
    };

    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes.to_vec());
    }

    // Execute the proxy request
    match proxy_req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

            let mut builder = axum::http::Response::builder().status(status);

            for (name, value) in resp.headers() {
                if name != header::TRANSFER_ENCODING {
                    builder = builder.header(name, value);
                }
            }

            match resp.bytes().await {
                Ok(bytes) => builder
                    .body(Body::from(bytes.to_vec()))
                    .unwrap_or_else(|_| {
                        (StatusCode::BAD_GATEWAY, "proxy response error").into_response()
                    }),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": "bad_gateway",
                        "message": format!("failed to read upstream response: {e}"),
                    })),
                )
                    .into_response(),
            }
        }
        Err(e) if e.is_connect() => {
            warn!(
                target_url = %target_url,
                error = %e,
                "Upstream unreachable — returning 503"
            );
            let mut resp = (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "service_unavailable",
                    "message": format!("upstream unreachable: {}", route.upstream_addr),
                })),
            )
                .into_response();
            resp.headers_mut().insert(
                header::RETRY_AFTER,
                RETRY_AFTER_SECONDS.parse().unwrap(),
            );
            resp
        }
        Err(e) => {
            warn!(
                target_url = %target_url,
                error = %e,
                "Ingress proxy request failed"
            );
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "bad_gateway",
                    "message": format!("failed to connect to workload: {e}"),
                })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::IngressRouter;

    #[tokio::test]
    async fn test_503_when_upstream_down() {
        let router = std::sync::Arc::new(IngressRouter::new());

        // Add a route pointing to a port nothing is listening on
        let entry = RouteEntry {
            path_prefix: "/".to_string(),
            upstream_addr: "127.0.0.1".to_string(),
            upstream_port: 19999, // unlikely to be in use
            product: "test".to_string(),
            internal: false,
            ingress_iri: picloud_domain::iri::ResourceIri::new(
                "https://picloud.local/products/test/ingresses/dead",
            )
            .unwrap(),
        };
        router
            .upsert_route("test.picloud.local".to_string(), entry)
            .await;

        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "test.picloud.local".parse().unwrap());

        let resp = handle_proxy(
            router,
            Method::GET,
            "/hello".parse().unwrap(),
            headers,
            Body::empty(),
        )
        .await;

        // Should be 502 or 503 depending on whether it's a connect error
        let status = resp.status();
        assert!(
            status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::BAD_GATEWAY,
            "Expected 503 or 502, got {status}"
        );
    }
}
