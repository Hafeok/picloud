/// HTTP Server Implementation
///
/// Provides the axum-based HTTP server, IRI routing, and content negotiation
/// for the PiCloud platform. Every resource has a dereferenceable IRI that
/// maps directly to an HTTP route.
///
/// This crate depends only on picloud-domain. Trait objects for event log,
/// state projection, etc. are injected from the composition root.
use std::net::SocketAddr;

use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream;
use picloud_domain::iri::{ClusterDomain, IriBuilder};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tracing::info;

// ---------------------------------------------------------------------------
// Content negotiation
// ---------------------------------------------------------------------------

/// Supported response content types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Json,
    Turtle,
    JsonLd,
}

impl ContentType {
    /// Parse an `Accept` header value into a `ContentType`.
    /// Falls back to JSON when no known type is matched.
    pub fn from_accept(accept: &str) -> Self {
        // Iterate through comma-separated media ranges
        for part in accept.split(',') {
            let media = part.trim().split(';').next().unwrap_or("").trim();
            match media {
                "text/turtle" => return Self::Turtle,
                "application/ld+json" => return Self::JsonLd,
                "application/json" => return Self::Json,
                _ => {}
            }
        }
        Self::Json
    }

    /// The HTTP Content-Type header value for this type.
    pub fn as_header_value(&self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Turtle => "text/turtle",
            Self::JsonLd => "application/ld+json",
        }
    }
}

/// Extract `ContentType` from request headers.
fn content_type_from_headers(headers: &HeaderMap) -> ContentType {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(ContentType::from_accept)
        .unwrap_or(ContentType::Json)
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

/// Wrap a JSON-serialisable value in a response with the correct Content-Type
/// header based on the negotiated `ContentType`.
///
/// For now all formats emit JSON, but the Content-Type header is set correctly
/// so clients can distinguish the intended format.
pub fn resource_response(
    body: serde_json::Value,
    content_type: ContentType,
) -> Response {
    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type.as_header_value())],
        body_bytes,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The PiCloud HTTP server.
pub struct PiCloudHttpServer {
    pub bind_addr: SocketAddr,
    pub cluster_domain: ClusterDomain,
}

impl PiCloudHttpServer {
    pub fn new(bind_addr: SocketAddr, cluster_domain: ClusterDomain) -> Self {
        Self {
            bind_addr,
            cluster_domain,
        }
    }

    /// Build the axum [`Router`] with all platform routes.
    pub fn build_router(&self) -> Router {
        let iri_builder = IriBuilder::new(self.cluster_domain.clone());
        let cluster_root_iri = iri_builder.cluster_root().to_string();

        Router::new()
            .route("/", get(handle_cluster_root))
            .route("/health", get(handle_health))
            .route("/nodes", get(handle_nodes))
            .route("/nodes/:name", get(handle_node))
            .route("/products", get(handle_products))
            .route("/products/:name", get(handle_product))
            .route(
                "/products/:name/:resource_type/:resource_name",
                get(handle_resource),
            )
            .route("/products/:name/graph", get(handle_graph))
            .route("/products/:name/events", get(handle_events))
            .route("/api/commands", post(handle_command))
            .with_state(AppState {
                cluster_root_iri,
                cluster_domain: self.cluster_domain.clone(),
            })
    }

    /// Bind to the configured address and start serving.
    pub async fn start(self) -> picloud_domain::error::Result<()> {
        let router = self.build_router();
        let listener = TcpListener::bind(self.bind_addr).await.map_err(|e| {
            picloud_domain::error::PiCloudError::Internal(
                format!("failed to bind to {}: {e}", self.bind_addr),
            )
        })?;
        info!("PiCloud HTTP server listening on {}", self.bind_addr);
        axum::serve(listener, router).await.map_err(|e| {
            picloud_domain::error::PiCloudError::Internal(
                format!("HTTP server error: {e}"),
            )
        })?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    cluster_root_iri: String,
    cluster_domain: ClusterDomain,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_cluster_root(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": state.cluster_root_iri,
            "type": "PiCloudCluster",
            "domain": state.cluster_domain.0,
        }),
        ct,
    )
}

async fn handle_health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn handle_nodes(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": iri_builder.cluster_root().to_string(),
            "type": "NodeList",
            "nodes": [],
        }),
        ct,
    )
}

async fn handle_node(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": iri_builder.node(&name).to_string(),
            "type": "Node",
            "name": name,
        }),
        ct,
    )
}

async fn handle_products(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": iri_builder.cluster_root().to_string(),
            "type": "ProductList",
            "products": [],
        }),
        ct,
    )
}

async fn handle_product(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": iri_builder.product(&name).to_string(),
            "type": "Product",
            "name": name,
        }),
        ct,
    )
}

async fn handle_resource(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((product, resource_type, resource_name)): Path<(String, String, String)>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": iri_builder.resource(&product, &resource_type, &resource_name).to_string(),
            "type": resource_type,
            "name": resource_name,
            "product": product,
        }),
        ct,
    )
}

#[derive(Deserialize)]
struct GraphQuery {
    query: Option<String>,
}

async fn handle_graph(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<GraphQuery>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let ct = content_type_from_headers(&headers);
    resource_response(
        serde_json::json!({
            "@id": iri_builder.product_graph(&name).to_string(),
            "type": "SparqlEndpoint",
            "product": name,
            "query": params.query.unwrap_or_default(),
            "results": [],
        }),
        ct,
    )
}

async fn handle_events(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let iri_builder = IriBuilder::new(state.cluster_domain);
    let event_iri = iri_builder.product_events(&name).to_string();

    // Placeholder: emit a single connected event then keep alive
    let initial = Event::default()
        .event("connected")
        .data(serde_json::json!({ "stream": event_iri }).to_string());

    Sse::new(stream::once(async { Ok(initial) })).keep_alive(KeepAlive::default())
}

#[derive(Deserialize, Serialize)]
struct CommandPayload {
    #[serde(flatten)]
    extra: serde_json::Value,
}

async fn handle_command(Json(payload): Json<CommandPayload>) -> impl IntoResponse {
    // Placeholder — the composition root will inject the real EventLog
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "accepted",
            "payload": payload.extra,
        })),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    fn test_server() -> PiCloudHttpServer {
        PiCloudHttpServer::new(
            "127.0.0.1:0".parse().unwrap(),
            ClusterDomain::default(),
        )
    }

    #[tokio::test]
    async fn router_builds_without_panic() {
        let _router = test_server().build_router();
    }

    #[tokio::test]
    async fn health_returns_200() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cluster_root_returns_json_by_default() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "application/json"
        );
    }

    #[tokio::test]
    async fn cluster_root_returns_turtle_content_type() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/")
            .header(header::ACCEPT, "text/turtle")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "text/turtle"
        );
    }

    #[tokio::test]
    async fn cluster_root_returns_jsonld_content_type() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/")
            .header(header::ACCEPT, "application/ld+json")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "application/ld+json"
        );
    }

    #[test]
    fn content_type_negotiation_parsing() {
        assert_eq!(ContentType::from_accept("application/json"), ContentType::Json);
        assert_eq!(ContentType::from_accept("text/turtle"), ContentType::Turtle);
        assert_eq!(ContentType::from_accept("application/ld+json"), ContentType::JsonLd);
        // First matching type wins
        assert_eq!(
            ContentType::from_accept("text/turtle, application/json"),
            ContentType::Turtle
        );
        // Quality params are present but we pick first match
        assert_eq!(
            ContentType::from_accept("application/ld+json;q=0.9, text/turtle;q=1.0"),
            ContentType::JsonLd
        );
        // Unknown falls back to JSON
        assert_eq!(ContentType::from_accept("text/html"), ContentType::Json);
        assert_eq!(ContentType::from_accept("*/*"), ContentType::Json);
    }

    #[tokio::test]
    async fn node_returns_iri() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/nodes/pi-node-01")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["@id"],
            "https://picloud.local/nodes/pi-node-01"
        );
    }

    #[tokio::test]
    async fn resource_returns_iri() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/products/photo-app/containers/api-server")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["@id"],
            "https://picloud.local/products/photo-app/containers/api-server"
        );
    }

    #[tokio::test]
    async fn command_returns_accepted() {
        let app = test_server().build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/api/commands")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"action":"test"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }
}
