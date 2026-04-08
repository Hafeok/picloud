/// HTTP Server Implementation
///
/// Provides the axum-based HTTP server, IRI routing, and content negotiation
/// for the PiCloud platform. Every resource has a dereferenceable IRI that
/// maps directly to an HTTP route.
///
/// This crate depends only on picloud-domain. Trait objects for event log,
/// state projection, etc. are injected from the composition root.
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream;
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::parser::{ResourceDeclaration, ResourceFile};
use picloud_domain::traits::{
    ClusterMembership, EventFilter, EventLog, IdentityProvider, StateProjector,
    StorageBackend, WorkloadScheduler,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

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
// Ingress routing
// ---------------------------------------------------------------------------

/// An ingress route mapping a URL path prefix to a local workload port.
#[derive(Debug, Clone)]
pub struct IngressRoute {
    /// The URL path prefix that triggers this route (e.g. "/products/photo-app/api").
    pub path: String,
    /// The local port to proxy requests to (the workload's port on this node).
    pub target_port: u16,
    /// The product that owns this ingress route.
    pub product: String,
}

/// Thread-safe ingress routing table, shared between the HTTP handlers and the provisioner.
pub type IngressTable = Arc<RwLock<HashMap<String, IngressRoute>>>;

/// Create a new, empty ingress routing table.
pub fn new_ingress_table() -> IngressTable {
    Arc::new(RwLock::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// The PiCloud HTTP server.
pub struct PiCloudHttpServer {
    pub bind_addr: SocketAddr,
    pub cluster_domain: ClusterDomain,
    pub event_log: Option<Arc<dyn EventLog>>,
    pub projector: Option<Arc<dyn StateProjector>>,
    pub cluster: Option<Arc<dyn ClusterMembership>>,
    pub iam: Option<Arc<dyn IdentityProvider>>,
    pub storage: Option<Arc<dyn StorageBackend>>,
    pub scheduler: Option<Arc<dyn WorkloadScheduler>>,
    pub ingress_routes: IngressTable,
    pub tls_config: Option<rustls::ServerConfig>,
    pub extra_router: Option<Router>,
    pub otel_stream: Option<Arc<crate::otel::OtelStream>>,
    pub telemetry_store: Option<Arc<dyn picloud_domain::traits::TelemetryStore>>,
}

impl PiCloudHttpServer {
    pub fn new(bind_addr: SocketAddr, cluster_domain: ClusterDomain) -> Self {
        Self {
            bind_addr,
            cluster_domain,
            event_log: None,
            projector: None,
            cluster: None,
            iam: None,
            storage: None,
            scheduler: None,
            ingress_routes: new_ingress_table(),
            tls_config: None,
            extra_router: None,
            otel_stream: None,
            telemetry_store: None,
        }
    }

    /// Inject all platform dependencies.
    pub fn with_dependencies(
        mut self,
        event_log: Arc<dyn EventLog>,
        projector: Arc<dyn StateProjector>,
        cluster: Arc<dyn ClusterMembership>,
        iam: Arc<dyn IdentityProvider>,
        storage: Arc<dyn StorageBackend>,
        scheduler: Arc<dyn WorkloadScheduler>,
    ) -> Self {
        self.event_log = Some(event_log);
        self.projector = Some(projector);
        self.cluster = Some(cluster);
        self.iam = Some(iam);
        self.storage = Some(storage);
        self.scheduler = Some(scheduler);
        self
    }

    /// Set the ingress routing table (shared with the provisioner).
    pub fn with_ingress_table(mut self, table: IngressTable) -> Self {
        self.ingress_routes = table;
        self
    }

    /// Set a TLS configuration for HTTPS serving.
    pub fn with_tls_config(mut self, config: rustls::ServerConfig) -> Self {
        self.tls_config = Some(config);
        self
    }

    /// Merge additional routes into the HTTP server (e.g. Raft RPC endpoints).
    pub fn with_extra_router(mut self, router: Router) -> Self {
        self.extra_router = Some(router);
        self
    }

    /// Inject the OTel stream and telemetry store for the /otel and /telemetry endpoints (ADR-045, ADR-046).
    pub fn with_otel(
        mut self,
        otel_stream: Arc<crate::otel::OtelStream>,
        telemetry_store: Arc<dyn picloud_domain::traits::TelemetryStore>,
    ) -> Self {
        self.otel_stream = Some(otel_stream);
        self.telemetry_store = Some(telemetry_store);
        self
    }

    /// Build the axum [`Router`] with all platform routes.
    pub fn build_router(&self) -> Router {
        let iri_builder = IriBuilder::new(self.cluster_domain.clone());
        let cluster_root_iri = iri_builder.cluster_root().to_string();

        let mut router = Router::new()
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
            .route("/products/:name/ontology", get(handle_ontology))
            .route(
                "/products/:name/schemas/events/:event_type/v:version",
                get(handle_product_event_schema),
            )
            .route(
                "/products/:name/event-store/:store/:aggregate_type/:aggregate_id/events",
                get(handle_event_store_read).post(handle_event_store_append),
            )
            // --- Replay routes (ADR-035) ---
            .route(
                "/products/:name/event-store/:store/replay",
                post(handle_replay),
            )
            .route("/api/replay", post(handle_platform_replay))
            // --- Config store routes (ADR-043) ---
            .route("/products/:name/config", get(handle_config_list).post(handle_config_set))
            .route("/products/:name/config/:key", get(handle_config_get).delete(handle_config_delete))
            .route(
                "/products/:name/containers/:cname/config",
                get(handle_config_effective),
            )
            // --- Feature flag routes (ADR-044) ---
            .route("/products/:name/flags", get(handle_flags_list))
            .route("/products/:name/flags/:flag", get(handle_flag_get))
            // --- Inference rule routes (ADR-038) ---
            .route("/inference-rules", get(handle_inference_rules))
            // --- Group routes (ADR-037) ---
            .route("/groups", get(handle_groups))
            .route("/groups/:name", get(handle_group))
            .route("/api/commands", post(handle_command))
            .route("/api/apply", post(handle_apply))
            .route("/api/delete", post(handle_delete))
            .route("/api/tags/add", post(handle_tag_add))
            .route("/api/tags/remove", post(handle_tag_remove))
            .route("/api/tags", get(handle_tag_list))
            .route("/api/tags/find", get(handle_tag_find))
            .route(
                "/schemas/events/:event_type/v:version",
                get(handle_event_schema),
            )
            .route("/graph", get(handle_cluster_graph))
            .route(
                "/.well-known/openid-configuration",
                get(handle_oidc_discovery),
            )
            .route("/.well-known/jwks.json", get(handle_jwks))
            .route("/auth/token", post(handle_token))
            .route("/auth/authorize", get(handle_authorize))
            .route("/auth/register/begin", post(handle_register_begin))
            .route("/auth/register/complete", post(handle_register_complete))
            .route("/auth/login/begin", post(handle_login_begin))
            .route("/auth/login/complete", post(handle_login_complete))
            .route("/auth/enroll", post(handle_enroll))
            .route("/auth/device/begin", post(handle_device_begin))
            .route("/auth/device/poll", post(handle_device_poll))
            // --- OTel / Telemetry routes (ADR-045, ADR-046) ---
            .route("/otel", post(handle_otel_ingest))
            .route("/telemetry/spans", get(handle_telemetry_spans))
            .route("/telemetry/metrics", get(handle_telemetry_metrics))
            .fallback(handle_ingress_proxy)
            .with_state(AppState {
                cluster_root_iri,
                cluster_domain: self.cluster_domain.clone(),
                event_log: self.event_log.clone(),
                projector: self.projector.clone(),
                cluster: self.cluster.clone(),
                iam: self.iam.clone(),
                ingress_routes: self.ingress_routes.clone(),
                otel_stream: self.otel_stream.clone(),
                telemetry_store: self.telemetry_store.clone(),
            });

        // Merge any extra routes (e.g. Raft RPC endpoints from picloud-cluster)
        if let Some(extra) = &self.extra_router {
            router = router.merge(extra.clone());
        }

        router
    }

    /// Bind to the configured address and start serving.
    ///
    /// If `tls_config` is set, serves HTTPS via `axum_server` with rustls.
    /// Otherwise serves plain HTTP (the default).
    pub async fn start(self) -> picloud_domain::error::Result<()> {
        let router = self.build_router();

        if let Some(tls_config) = self.tls_config {
            info!("PiCloud HTTPS server listening on {}", self.bind_addr);
            let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(
                Arc::new(tls_config),
            );
            axum_server::bind_rustls(self.bind_addr, rustls_config)
                .serve(router.into_make_service())
                .await
                .map_err(|e| {
                    picloud_domain::error::PiCloudError::Internal(
                        format!("HTTPS server error: {e}"),
                    )
                })?;
        } else {
            info!("PiCloud HTTP server listening on {}", self.bind_addr);
            let listener = TcpListener::bind(self.bind_addr).await.map_err(|e| {
                picloud_domain::error::PiCloudError::Internal(
                    format!("failed to bind to {}: {e}", self.bind_addr),
                )
            })?;
            axum::serve(listener, router).await.map_err(|e| {
                picloud_domain::error::PiCloudError::Internal(
                    format!("HTTP server error: {e}"),
                )
            })?;
        }
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
    event_log: Option<Arc<dyn EventLog>>,
    projector: Option<Arc<dyn StateProjector>>,
    cluster: Option<Arc<dyn ClusterMembership>>,
    iam: Option<Arc<dyn IdentityProvider>>,
    ingress_routes: IngressTable,
    otel_stream: Option<Arc<crate::otel::OtelStream>>,
    telemetry_store: Option<Arc<dyn picloud_domain::traits::TelemetryStore>>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_cluster_root(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let ct = content_type_from_headers(&headers);

    // Query real cluster members if available
    let nodes = if let Some(ref cluster) = state.cluster {
        match cluster.members().await {
            Ok(members) => members
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "@id": m.node_iri.as_str(),
                        "nodeId": m.node_id.to_string(),
                        "address": m.address,
                        "isLeader": m.is_leader,
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": state.cluster_root_iri,
            "type": "PiCloudCluster",
            "domain": state.cluster_domain.0,
            "nodes": nodes,
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
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);

    let nodes = if let Some(ref cluster) = state.cluster {
        match cluster.members().await {
            Ok(members) => members
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "@id": m.node_iri.as_str(),
                        "nodeId": m.node_id.to_string(),
                        "address": m.address,
                        "isLeader": m.is_leader,
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": iri_builder.cluster_root().to_string(),
            "type": "NodeList",
            "nodes": nodes,
        }),
        ct,
    )
}

async fn handle_node(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);

    // Try to find the node in real cluster state
    let node_iri = iri_builder.node(&name);
    let node_data = if let Some(ref cluster) = state.cluster {
        match cluster.members().await {
            Ok(members) => members.iter().find(|m| m.node_iri == node_iri).map(|m| {
                serde_json::json!({
                    "@id": m.node_iri.as_str(),
                    "type": "Node",
                    "nodeId": m.node_id.to_string(),
                    "name": name,
                    "address": m.address,
                    "isLeader": m.is_leader,
                })
            }),
            Err(_) => None,
        }
    } else {
        None
    };

    match node_data {
        Some(data) => resource_response(data, ct),
        None => resource_response(
            serde_json::json!({
                "@id": node_iri.as_str(),
                "type": "Node",
                "name": name,
            }),
            ct,
        ),
    }
}

async fn handle_products(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let ct = content_type_from_headers(&headers);

    // Query the RDF graph for all products
    let products = if let Some(ref projector) = state.projector {
        match projector
            .query("SELECT ?product ?status WHERE { ?product <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://picloud.local/ontology#Resource> . ?product <https://picloud.local/ontology#resourceType> \"Product\" . OPTIONAL { ?product <https://picloud.local/ontology#status> ?status } }")
            .await
        {
            Ok(result) => result
                .bindings
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "@id": row["product"]["value"],
                        "status": row.get("status").and_then(|s| s.get("value")),
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": state.cluster_root_iri,
            "type": "ProductList",
            "products": products,
        }),
        ct,
    )
}

async fn handle_product(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let product_iri = iri_builder.product(&name);

    // Query RDF graph for product details
    let resources = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?res ?rtype ?status WHERE {{ ?res <https://picloud.local/ontology#resourceType> ?rtype . ?res <https://picloud.local/ontology#status> ?status . FILTER(STRSTARTS(STR(?res), \"{}/\")) }}",
            product_iri.as_str()
        );
        match projector.query(&sparql).await {
            Ok(result) => result
                .bindings
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "@id": row["res"]["value"],
                        "type": row["rtype"]["value"],
                        "status": row.get("status").and_then(|s| s.get("value")),
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": product_iri.as_str(),
            "type": "Product",
            "name": name,
            "resources": resources,
            "graph": iri_builder.product_graph(&name).as_str(),
            "events": iri_builder.product_events(&name).as_str(),
        }),
        ct,
    )
}

async fn handle_resource(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((product, resource_type, resource_name)): Path<(String, String, String)>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let resource_iri = iri_builder.resource(&product, &resource_type, &resource_name);

    // Try to get resource status from RDF graph
    let status = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?status WHERE {{ <{}> <https://picloud.local/ontology#status> ?status }}",
            resource_iri.as_str()
        );
        match projector.query(&sparql).await {
            Ok(result) => result
                .bindings
                .first()
                .and_then(|row| row["status"]["value"].as_str().map(String::from)),
            Err(_) => None,
        }
    } else {
        None
    };

    resource_response(
        serde_json::json!({
            "@id": resource_iri.as_str(),
            "type": resource_type,
            "name": resource_name,
            "product": product,
            "status": status.unwrap_or_else(|| "unknown".to_string()),
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
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);

    match params.query {
        Some(sparql) if !sparql.is_empty() => {
            // Execute real SPARQL query against the product graph
            if let Some(ref projector) = state.projector {
                let product_iri = iri_builder.product(&name);
                match projector.query_product(&product_iri, &sparql).await {
                    Ok(result) => resource_response(
                        serde_json::json!({
                            "@id": iri_builder.product_graph(&name).as_str(),
                            "type": "SparqlResult",
                            "product": name,
                            "results": result.bindings,
                        }),
                        ct,
                    ),
                    Err(e) => resource_response(
                        serde_json::json!({
                            "@id": iri_builder.product_graph(&name).as_str(),
                            "type": "SparqlError",
                            "error": e.to_string(),
                        }),
                        ct,
                    ),
                }
            } else {
                resource_response(
                    serde_json::json!({
                        "@id": iri_builder.product_graph(&name).as_str(),
                        "type": "SparqlEndpoint",
                        "error": "projector not available",
                    }),
                    ct,
                )
            }
        }
        _ => resource_response(
            serde_json::json!({
                "@id": iri_builder.product_graph(&name).as_str(),
                "type": "SparqlEndpoint",
                "product": name,
                "hint": "Provide a ?query= parameter with a SPARQL query",
            }),
            ct,
        ),
    }
}

type SseStream = std::pin::Pin<
    Box<dyn futures::Stream<Item = Result<Event, std::convert::Infallible>> + Send>,
>;

async fn handle_events(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Sse<SseStream> {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let event_iri = iri_builder.product_events(&name).to_string();

    // Subscribe to real event stream if event log is available
    if let Some(ref event_log) = state.event_log {
        let filter = EventFilter {
            product: Some(name.clone()),
            ..Default::default()
        };
        if let Ok(mut rx) = event_log.subscribe(filter).await {
            let stream: SseStream = Box::pin(async_stream::stream! {
                // Send connected event first
                yield Ok(Event::default()
                    .event("connected")
                    .data(serde_json::json!({ "stream": event_iri }).to_string()));

                loop {
                    match rx.recv().await {
                        Ok(envelope) => {
                            let data = serde_json::json!({
                                "id": envelope.id.to_string(),
                                "type": envelope.event_type,
                                "timestamp": envelope.timestamp.to_rfc3339(),
                                "source": envelope.source.as_str(),
                                "correlationId": envelope.correlation_id.to_string(),
                                "payload": envelope.payload,
                            });
                            yield Ok(Event::default()
                                .event(&envelope.event_type)
                                .data(data.to_string()));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            yield Ok(Event::default()
                                .event("lagged")
                                .data(format!("{{\"skipped\":{n}}}")));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            return Sse::new(stream).keep_alive(KeepAlive::default());
        }
    }

    // Fallback: emit a single connected event then keep alive
    let initial = Event::default()
        .event("connected")
        .data(serde_json::json!({ "stream": event_iri }).to_string());

    let stream: SseStream = Box::pin(stream::once(async { Ok(initial) }));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Deserialize, Serialize)]
struct CommandPayload {
    /// The event type, e.g. "ResourceDeclared"
    #[serde(rename = "type")]
    event_type: Option<String>,
    /// The source resource IRI
    source: Option<String>,
    /// The product scope (if applicable)
    product: Option<String>,
    /// Event payload data
    #[serde(default)]
    payload: serde_json::Value,
    /// Optional idempotency key
    idempotency_key: Option<String>,
}

async fn handle_command(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(cmd): Json<CommandPayload>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let event_type = cmd.event_type.unwrap_or_else(|| "Command".to_string());
    let correlation_id = Uuid::new_v4();
    let source_str = cmd
        .source
        .unwrap_or_else(|| format!("{}/api/commands", state.cluster_root_iri));

    let source = match ResourceIri::new(&source_str) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid source IRI: {e}") })),
            );
        }
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let schema = iri_builder.event_schema(&event_type, 1);

    let mut envelope = EventEnvelope::new(
        schema,
        &event_type,
        source,
        cmd.product,
        correlation_id,
        cmd.payload,
    );
    envelope.idempotency_key = cmd.idempotency_key;

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append command event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// Handle resource apply — parses a ResourceFile, emits events for each resource.
async fn handle_apply(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(resource_file): Json<ResourceFile>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    // Validate the resource file
    if let Err(e) = resource_file.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }

    // Sort resources in dependency order: products -> volumes -> containers/binaries -> rest
    let mut resource_file = resource_file;
    resource_file.sort_for_provisioning();

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let correlation_id = Uuid::new_v4();
    let mut results = Vec::new();

    for decl in &resource_file.resources {
        let (resource_iri, event_type, product, payload) = match decl {
            ResourceDeclaration::Product(p) => {
                let iri = iri_builder.product(&p.name);
                let payload = serde_json::json!({
                    "product_iri": iri.as_str(),
                    "product_name": p.name,
                    "version": p.version,
                    "description": p.description,
                });
                (iri, "ProductDeployed", None, payload)
            }
            ResourceDeclaration::Volume(v) => {
                let iri = iri_builder.resource(&v.product, "volumes", &v.name);
                let intent = v.storage_intent();
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Volume",
                    "product": v.product,
                    "name": v.name,
                    "size_gb": v.size_gb,
                    "durability": format!("{:?}", intent.durability),
                    "performance": format!("{:?}", intent.performance),
                });
                (iri, "ResourceDeclared", Some(v.product.clone()), payload)
            }
            ResourceDeclaration::Container(c) => {
                let iri = iri_builder.resource(&c.product, "containers", &c.name);
                let spec = c.to_spec();
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Container",
                    "product": c.product,
                    "name": c.name,
                    "image": spec.image,
                    "identity": spec.identity,
                });
                (iri, "ResourceDeclared", Some(c.product.clone()), payload)
            }
            ResourceDeclaration::Binary(b) => {
                let iri = iri_builder.resource(&b.product, "binaries", &b.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Binary",
                    "product": b.product,
                    "name": b.name,
                    "executable": b.executable,
                });
                (iri, "ResourceDeclared", Some(b.product.clone()), payload)
            }
            ResourceDeclaration::EventSubscription(e) => {
                let iri = iri_builder.resource(&e.product, "event-subscriptions", &e.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "EventSubscription",
                    "product": e.product,
                    "name": e.name,
                    "source": e.source,
                    "event": e.event,
                    "handler": e.handler,
                });
                (iri, "ResourceDeclared", Some(e.product.clone()), payload)
            }
            ResourceDeclaration::Ingress(i) => {
                let iri = iri_builder.resource(&i.product, "ingresses", &i.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Ingress",
                    "product": i.product,
                    "name": i.name,
                    "target": i.target,
                    "port": i.port,
                    "path": i.path,
                    "tls": i.tls,
                });
                (iri, "ResourceDeclared", Some(i.product.clone()), payload)
            }
            ResourceDeclaration::Secret(s) => {
                let iri = iri_builder.resource(&s.product, "secrets", &s.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Secret",
                    "product": s.product,
                    "name": s.name,
                });
                (iri, "ResourceDeclared", Some(s.product.clone()), payload)
            }
            ResourceDeclaration::Role(r) => {
                let product = r.product.clone().unwrap_or_default();
                let iri = if product.is_empty() {
                    iri_builder.resource("platform", "roles", &r.name)
                } else {
                    iri_builder.resource(&product, "roles", &r.name)
                };
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Role",
                    "product": r.product,
                    "name": r.name,
                    "permissions": r.permissions,
                });
                (iri, "ResourceDeclared", r.product.clone(), payload)
            }
            ResourceDeclaration::Config(c) => {
                let iri = iri_builder.product_config(&c.product);
                let entries: Vec<serde_json::Value> = c.entries.iter().map(|e| {
                    serde_json::json!({
                        "key": e.key,
                        "value": e.value,
                        "type": e.config_type,
                        "tags": e.tags,
                    })
                }).collect();
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "ConfigStore",
                    "product": c.product,
                    "name": c.name,
                    "entries": entries,
                });
                (iri, "ResourceDeclared", Some(c.product.clone()), payload)
            }
            ResourceDeclaration::FeatureFlag(f) => {
                let iri = iri_builder.feature_flag(&f.product, &f.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "FeatureFlag",
                    "product": f.product,
                    "name": f.name,
                    "description": f.description,
                    "enabled": f.enabled,
                    "version": f.version,
                });
                (iri, "ResourceDeclared", Some(f.product.clone()), payload)
            }
            ResourceDeclaration::Group(g) => {
                let iri = iri_builder.group(&g.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Group",
                    "name": g.name,
                    "description": g.description,
                    "roles": g.roles,
                });
                (iri, "ResourceDeclared", None, payload)
            }
            ResourceDeclaration::InferenceRule(r) => {
                let iri = iri_builder.inference_rule(&r.name);
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "InferenceRule",
                    "name": r.name,
                    "description": r.description,
                    "scope": r.scope,
                    "trigger": r.trigger,
                    "trigger_events": r.trigger_events,
                    "reconciliation": r.reconciliation,
                    "construct_query": r.construct,
                });
                (iri, "ResourceDeclared", None, payload)
            }
        };

        let schema = iri_builder.event_schema(event_type, 1);
        let idempotency_key = format!(
            "apply-{}-{}-{}",
            decl.resource_type(),
            decl.resource_name(),
            correlation_id
        );
        let envelope = EventEnvelope::new(
            schema,
            event_type,
            resource_iri,
            product,
            correlation_id,
            payload,
        )
        .with_idempotency_key(idempotency_key);

        match event_log.append(envelope).await {
            Ok(()) => {
                results.push(serde_json::json!({
                    "name": decl.resource_name(),
                    "type": decl.resource_type(),
                    "status": "declared",
                }));

                // Emit TagAdded events for each tag on the resource (ADR-036)
                let resource_iri_str = match decl {
                    ResourceDeclaration::Product(p) => iri_builder.product(&p.name).as_str().to_string(),
                    ResourceDeclaration::Volume(v) => iri_builder.resource(&v.product, "volumes", &v.name).as_str().to_string(),
                    ResourceDeclaration::Container(c) => iri_builder.resource(&c.product, "containers", &c.name).as_str().to_string(),
                    ResourceDeclaration::Binary(b) => iri_builder.resource(&b.product, "binaries", &b.name).as_str().to_string(),
                    ResourceDeclaration::EventSubscription(e) => iri_builder.resource(&e.product, "event-subscriptions", &e.name).as_str().to_string(),
                    ResourceDeclaration::Ingress(i) => iri_builder.resource(&i.product, "ingresses", &i.name).as_str().to_string(),
                    ResourceDeclaration::Secret(s) => iri_builder.resource(&s.product, "secrets", &s.name).as_str().to_string(),
                    ResourceDeclaration::Role(r) => {
                        let p = r.product.as_deref().unwrap_or("platform");
                        iri_builder.resource(p, "roles", &r.name).as_str().to_string()
                    }
                    ResourceDeclaration::Config(c) => iri_builder.product_config(&c.product).as_str().to_string(),
                    ResourceDeclaration::FeatureFlag(f) => iri_builder.feature_flag(&f.product, &f.name).as_str().to_string(),
                    ResourceDeclaration::Group(g) => iri_builder.group(&g.name).as_str().to_string(),
                    ResourceDeclaration::InferenceRule(r) => iri_builder.inference_rule(&r.name).as_str().to_string(),
                };

                for (tag_key, tag_value) in decl.tags() {
                    let tag_schema = iri_builder.event_schema("TagAdded", 1);
                    let tag_source = ResourceIri(resource_iri_str.clone());
                    let tag_envelope = EventEnvelope::new(
                        tag_schema,
                        "TagAdded",
                        tag_source,
                        decl.product_name().map(String::from),
                        correlation_id,
                        serde_json::json!({
                            "resource_iri": resource_iri_str,
                            "key": tag_key,
                            "value": tag_value,
                        }),
                    );
                    if let Err(e) = event_log.append(tag_envelope).await {
                        warn!(error = %e, key = %tag_key, "Failed to append TagAdded event");
                    }
                }
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "name": decl.resource_name(),
                    "type": decl.resource_type(),
                    "status": "failed",
                    "error": e.to_string(),
                }));
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "correlationId": correlation_id.to_string(),
            "results": results,
        })),
    )
}

/// Request payload for the /api/delete endpoint.
#[derive(Deserialize)]
struct DeletePayload {
    /// Name of the product to delete
    product: String,
}

/// Handle product deletion — emits a ProductDeleted event to trigger cascading deletion.
async fn handle_delete(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(payload): Json<DeletePayload>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let product_iri = iri_builder.product(&payload.product);
    let correlation_id = Uuid::new_v4();

    let schema = iri_builder.event_schema("ProductDeleted", 1);
    let envelope = EventEnvelope::new(
        schema,
        "ProductDeleted",
        product_iri.clone(),
        Some(payload.product.clone()),
        correlation_id,
        serde_json::json!({
            "product_iri": product_iri.as_str(),
            "product_name": payload.product,
        }),
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "product": payload.product,
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append ProductDeleted event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Tag endpoints (ADR-036)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TagAddPayload {
    resource_iri: String,
    key: String,
    value: String,
    /// Product scope (optional)
    product: Option<String>,
}

/// POST /api/tags/add — add a tag to a resource
async fn handle_tag_add(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(payload): Json<TagAddPayload>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let correlation_id = Uuid::new_v4();

    let source = match ResourceIri::new(&payload.resource_iri) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid resource IRI: {e}") })),
            );
        }
    };

    let schema = iri_builder.event_schema("TagAdded", 1);
    let envelope = EventEnvelope::new(
        schema,
        "TagAdded",
        source,
        payload.product,
        correlation_id,
        serde_json::json!({
            "resource_iri": payload.resource_iri,
            "key": payload.key,
            "value": payload.value,
        }),
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append TagAdded event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

#[derive(Deserialize)]
struct TagRemovePayload {
    resource_iri: String,
    key: String,
    value: String,
    product: Option<String>,
}

/// POST /api/tags/remove — remove a tag from a resource
async fn handle_tag_remove(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(payload): Json<TagRemovePayload>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let correlation_id = Uuid::new_v4();

    let source = match ResourceIri::new(&payload.resource_iri) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid resource IRI: {e}") })),
            );
        }
    };

    let schema = iri_builder.event_schema("TagRemoved", 1);
    let envelope = EventEnvelope::new(
        schema,
        "TagRemoved",
        source,
        payload.product,
        correlation_id,
        serde_json::json!({
            "resource_iri": payload.resource_iri,
            "key": payload.key,
            "value": payload.value,
        }),
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append TagRemoved event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

#[derive(Deserialize)]
struct TagListQuery {
    resource: String,
}

/// GET /api/tags?resource=<iri> — list all tags on a resource
async fn handle_tag_list(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(params): Query<TagListQuery>,
) -> impl IntoResponse {
    let Some(ref projector) = state.projector else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "projector not available" })),
        );
    };

    let sparql = format!(
        "SELECT ?key ?value WHERE {{ <{}> <https://picloud.local/ontology#tag> ?tag . ?tag <https://picloud.local/ontology#tagKey> ?key . ?tag <https://picloud.local/ontology#tagValue> ?value }}",
        params.resource
    );

    match projector.query(&sparql).await {
        Ok(result) => {
            let tags: Vec<serde_json::Value> = result
                .bindings
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "key": row["key"]["value"],
                        "value": row["value"]["value"],
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "resource": params.resource,
                    "tags": tags,
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

#[derive(Deserialize)]
struct TagFindQuery {
    key: String,
    value: String,
}

/// GET /api/tags/find?key=<k>&value=<v> — find all resources with a specific tag
async fn handle_tag_find(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(params): Query<TagFindQuery>,
) -> impl IntoResponse {
    let Some(ref projector) = state.projector else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "projector not available" })),
        );
    };

    let sparql = format!(
        "SELECT ?resource WHERE {{ ?resource <https://picloud.local/ontology#tag> ?tag . ?tag <https://picloud.local/ontology#tagKey> \"{}\" . ?tag <https://picloud.local/ontology#tagValue> \"{}\" }}",
        params.key, params.value
    );

    match projector.query(&sparql).await {
        Ok(result) => {
            let resources: Vec<serde_json::Value> = result
                .bindings
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "@id": row["resource"]["value"],
                    })
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "tag": format!("{}={}", params.key, params.value),
                    "resources": resources,
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Return a JSON Schema document describing a platform event payload.
/// Route: GET /schemas/events/:event_type/v:version
async fn handle_event_schema(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((event_type, version)): Path<(String, String)>,
) -> Response {
    let ct = content_type_from_headers(&headers);
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());

    let ver: u32 = match version.parse() {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "version must be a positive integer" })),
            )
                .into_response();
        }
    };

    let schema_iri = iri_builder.event_schema(&event_type, ver);

    resource_response(
        serde_json::json!({
            "$id": schema_iri.as_str(),
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": event_type,
            "description": format!("Schema for {} event (version {})", event_type, ver),
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "schema": { "type": "string", "format": "iri" },
                "event_type": { "type": "string", "const": event_type },
                "timestamp": { "type": "string", "format": "date-time" },
                "source": { "type": "string", "format": "iri" },
                "product": { "type": ["string", "null"] },
                "correlation_id": { "type": "string", "format": "uuid" },
                "idempotency_key": { "type": ["string", "null"] },
                "payload": { "type": "object" }
            },
            "required": ["id", "schema", "event_type", "timestamp", "source", "correlation_id", "payload"],
        }),
        ct,
    )
}

/// Return a JSON Schema document for a product-specific event.
/// Route: GET /products/:name/schemas/events/:event_type/v:version
async fn handle_product_event_schema(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((product, event_type, version)): Path<(String, String, String)>,
) -> Response {
    let ct = content_type_from_headers(&headers);
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());

    let ver: u32 = match version.parse() {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "version must be a positive integer" })),
            )
                .into_response();
        }
    };

    let schema_iri = iri_builder.product_event_schema(&product, &event_type, ver);
    let product_iri = iri_builder.product(&product);

    resource_response(
        serde_json::json!({
            "$id": schema_iri.as_str(),
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": format!("{}/{}", product, event_type),
            "description": format!("Schema for {} event in product {} (version {})", event_type, product, ver),
            "type": "object",
            "properties": {
                "id": { "type": "string", "format": "uuid" },
                "schema": { "type": "string", "format": "iri" },
                "event_type": { "type": "string", "const": event_type },
                "timestamp": { "type": "string", "format": "date-time" },
                "source": { "type": "string", "format": "iri" },
                "product": { "type": "string", "const": product },
                "correlation_id": { "type": "string", "format": "uuid" },
                "idempotency_key": { "type": ["string", "null"] },
                "payload": { "type": "object" }
            },
            "required": ["id", "schema", "event_type", "timestamp", "source", "product", "correlation_id", "payload"],
            "x-picloud-product": product_iri.as_str(),
        }),
        ct,
    )
}

/// Serve a product's ontology (Turtle format).
/// Route: GET /products/:name/ontology
async fn handle_ontology(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let ontology_iri = iri_builder.product_ontology(&name);

    // Try to look up the ontology resource from the RDF graph
    if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?format ?content WHERE {{ <{}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://picloud.local/ontology#Resource> . <{}> <https://picloud.local/ontology#resourceType> \"Ontology\" . OPTIONAL {{ <{}> <https://picloud.local/ontology#format> ?format }} . OPTIONAL {{ <{}> <https://picloud.local/ontology#content> ?content }} }}",
            ontology_iri.as_str(), ontology_iri.as_str(), ontology_iri.as_str(), ontology_iri.as_str()
        );
        if let Ok(result) = projector.query(&sparql).await {
            if let Some(row) = result.bindings.first() {
                let content = row
                    .get("content")
                    .and_then(|v| v.get("value"))
                    .and_then(|v| v.as_str());
                if let Some(turtle_content) = content {
                    // Serve raw Turtle if client accepts it, otherwise wrap in JSON
                    if ct == ContentType::Turtle {
                        return (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "text/turtle")],
                            turtle_content.to_string(),
                        )
                            .into_response();
                    }
                    return resource_response(
                        serde_json::json!({
                            "@id": ontology_iri.as_str(),
                            "type": "Ontology",
                            "product": name,
                            "format": "turtle",
                            "content": turtle_content,
                        }),
                        ct,
                    );
                }
            }
        }
    }

    // Ontology not yet loaded or projector unavailable — return metadata stub
    resource_response(
        serde_json::json!({
            "@id": ontology_iri.as_str(),
            "type": "Ontology",
            "product": name,
            "status": "not_loaded",
            "hint": "Declare an Ontology resource in your .picloud file to serve it here",
        }),
        ct,
    )
}

/// Handle SPARQL query against the cluster-level graph (not product-scoped).
async fn handle_cluster_graph(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(params): Query<GraphQuery>,
) -> Response {
    let ct = content_type_from_headers(&headers);

    match params.query {
        Some(sparql) if !sparql.is_empty() => {
            if let Some(ref projector) = state.projector {
                match projector.query(&sparql).await {
                    Ok(result) => resource_response(
                        serde_json::json!({
                            "type": "SparqlResult",
                            "results": result.bindings,
                        }),
                        ct,
                    ),
                    Err(e) => resource_response(
                        serde_json::json!({
                            "type": "SparqlError",
                            "error": e.to_string(),
                        }),
                        ct,
                    ),
                }
            } else {
                resource_response(
                    serde_json::json!({
                        "type": "SparqlEndpoint",
                        "error": "projector not available",
                    }),
                    ct,
                )
            }
        }
        _ => resource_response(
            serde_json::json!({
                "type": "SparqlEndpoint",
                "hint": "Provide a ?query= parameter with a SPARQL query",
            }),
            ct,
        ),
    }
}

// ---------------------------------------------------------------------------
// Ingress proxy handler
// ---------------------------------------------------------------------------

/// Catch-all handler that checks the ingress routing table and proxies
/// matching requests to the workload's local port.
async fn handle_ingress_proxy(
    axum::extract::State(state): axum::extract::State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let request_path = uri.path();

    // Find the longest matching path prefix in the ingress table
    let route = {
        let routes = state.ingress_routes.read().await;
        routes
            .values()
            .filter(|r| request_path.starts_with(&r.path))
            .max_by_key(|r| r.path.len())
            .cloned()
    };

    let Some(route) = route else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "not_found",
            "message": format!("no route matches path: {request_path}"),
        })))
            .into_response();
    };

    // Strip the ingress path prefix and forward the remainder to the workload
    let downstream_path = &request_path[route.path.len()..];
    let downstream_path = if downstream_path.is_empty() || !downstream_path.starts_with('/') {
        format!("/{downstream_path}")
    } else {
        downstream_path.to_string()
    };

    let query = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_url = format!(
        "http://127.0.0.1:{}{downstream_path}{query}",
        route.target_port
    );

    // Build the proxied request
    let client = reqwest::Client::new();
    let mut proxy_req = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
        &target_url,
    );

    // Forward relevant headers (skip host — we're proxying to localhost)
    for (name, value) in headers.iter() {
        if name != header::HOST && name != header::TRANSFER_ENCODING {
            if let Ok(v) = value.to_str() {
                proxy_req = proxy_req.header(name.as_str(), v);
            }
        }
    }

    // Forward the body
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "bad_request",
                "message": format!("failed to read request body: {e}"),
            })))
                .into_response();
        }
    };

    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes.to_vec());
    }

    // Execute the proxy request
    match proxy_req.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(StatusCode::BAD_GATEWAY);

            let mut builder = axum::http::Response::builder().status(status);

            for (name, value) in resp.headers() {
                // Skip transfer-encoding as we're re-encoding the body
                if name != header::TRANSFER_ENCODING {
                    builder = builder.header(name, value);
                }
            }

            match resp.bytes().await {
                Ok(bytes) => builder
                    .body(axum::body::Body::from(bytes.to_vec()))
                    .unwrap_or_else(|_| {
                        (StatusCode::BAD_GATEWAY, "proxy response error").into_response()
                    }),
                Err(e) => (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                    "error": "bad_gateway",
                    "message": format!("failed to read upstream response: {e}"),
                })))
                    .into_response(),
            }
        }
        Err(e) => {
            warn!(
                target_url = %target_url,
                error = %e,
                "Ingress proxy request failed"
            );
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                "error": "bad_gateway",
                "message": format!("failed to connect to workload: {e}"),
            })))
                .into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Product Event Store Handlers
// ---------------------------------------------------------------------------

/// Request body for appending an event to a product event store.
#[derive(Deserialize)]
struct EventStoreAppendRequest {
    /// Schema IRI for the event (e.g. "https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v1")
    schema: String,
    /// The event type name (e.g. "OrderPlaced")
    #[serde(rename = "type")]
    event_type: String,
    /// The event payload
    payload: serde_json::Value,
}

/// POST /products/:name/event-store/:store/:aggregate_type/:aggregate_id/events
///
/// Append an event to a product's aggregate stream.
async fn handle_event_store_append(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((product, store, aggregate_type, aggregate_id)): Path<(String, String, String, String)>,
    Json(body): Json<EventStoreAppendRequest>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());

    // Build the source IRI for this aggregate stream
    let source = iri_builder.aggregate_stream(&product, &store, &aggregate_type, &aggregate_id);

    // Parse the schema IRI
    let schema = match ResourceIri::new(&body.schema) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid schema IRI: {e}") })),
            );
        }
    };

    let correlation_id = Uuid::new_v4();
    let envelope = EventEnvelope::new(
        schema,
        &body.event_type,
        source,
        Some(product.clone()),
        correlation_id,
        body.payload,
    );
    let event_id = envelope.id;

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "eventId": event_id.to_string(),
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append event store event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// GET /products/:name/event-store/:store/:aggregate_type/:aggregate_id/events
///
/// Read all events for a product's aggregate stream.
async fn handle_event_store_read(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((product, store, aggregate_type, aggregate_id)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let stream_iri = iri_builder.aggregate_stream(&product, &store, &aggregate_type, &aggregate_id);
    let stream_iri_str = stream_iri.as_str().to_string();

    // Get all events from offset 0, then filter by product + source IRI
    let all_events = event_log.events_since(0).await;
    let matching: Vec<serde_json::Value> = all_events
        .into_iter()
        .filter(|e| e.product.as_deref() == Some(&product) && e.source.as_str() == stream_iri_str)
        .map(|e| {
            serde_json::json!({
                "id": e.id.to_string(),
                "schema": e.schema.as_str(),
                "type": e.event_type,
                "timestamp": e.timestamp.to_rfc3339(),
                "source": e.source.as_str(),
                "correlationId": e.correlation_id.to_string(),
                "payload": e.payload,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "stream": stream_iri_str,
            "product": product,
            "store": store,
            "aggregateType": aggregate_type,
            "aggregateId": aggregate_id,
            "events": matching,
        })),
    )
}

// ---------------------------------------------------------------------------
// Replay Handlers (ADR-035)
// ---------------------------------------------------------------------------

/// Request body for POST /products/:name/event-store/:store/replay
#[derive(Deserialize)]
struct ReplayRequestBody {
    from: String,
    to: Option<String>,
    aggregate_type: Option<String>,
    aggregate_ids: Option<Vec<String>>,
}

/// POST /products/:name/event-store/:store/replay
///
/// Trigger a replay of events from a product's event store.
/// Accepts from/to timestamps, optional aggregate_type and aggregate_ids.
/// Returns a replay_id and emits ReplayStarted event.
async fn handle_replay(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((product, store)): Path<(String, String)>,
    Json(body): Json<ReplayRequestBody>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    // Parse timestamps
    let from = match body.from.parse::<chrono::DateTime<chrono::Utc>>() {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid 'from' timestamp: {e}") })),
            );
        }
    };
    let to = match body.to {
        Some(ref s) => match s.parse::<chrono::DateTime<chrono::Utc>>() {
            Ok(t) => Some(t),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid 'to' timestamp: {e}") })),
                );
            }
        },
        None => None,
    };

    let replay_id = Uuid::new_v4();
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let aggregate_ids = body.aggregate_ids.unwrap_or_default();

    // Emit ReplayStarted event
    let source = iri_builder.product(&product);
    let schema = iri_builder.event_schema("ReplayStarted", 1);
    let payload = serde_json::json!({
        "replay_id": replay_id.to_string(),
        "product": product,
        "store": store,
        "from": from.to_rfc3339(),
        "to": to.map(|t| t.to_rfc3339()),
        "aggregate_type": body.aggregate_type,
        "aggregate_ids": aggregate_ids,
    });

    let envelope = EventEnvelope::new(
        schema,
        "ReplayStarted",
        source,
        Some(product.clone()),
        Uuid::new_v4(),
        payload,
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "replay_id": replay_id.to_string(),
                "status": "started",
                "product": product,
                "store": store,
                "from": from.to_rfc3339(),
                "to": to.map(|t| t.to_rfc3339()),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to start replay");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// Request body for POST /api/replay (platform-level replay)
#[derive(Deserialize)]
struct PlatformReplayRequestBody {
    from: String,
    to: Option<String>,
}

/// POST /api/replay
///
/// Trigger a platform-level replay (cluster events).
async fn handle_platform_replay(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<PlatformReplayRequestBody>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let from = match body.from.parse::<chrono::DateTime<chrono::Utc>>() {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid 'from' timestamp: {e}") })),
            );
        }
    };
    let to = match body.to {
        Some(ref s) => match s.parse::<chrono::DateTime<chrono::Utc>>() {
            Ok(t) => Some(t),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid 'to' timestamp: {e}") })),
                );
            }
        },
        None => None,
    };

    let replay_id = Uuid::new_v4();
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());

    let source = iri_builder.cluster_root();
    let schema = iri_builder.event_schema("ReplayStarted", 1);
    let payload = serde_json::json!({
        "replay_id": replay_id.to_string(),
        "product": null,
        "from": from.to_rfc3339(),
        "to": to.map(|t| t.to_rfc3339()),
        "aggregate_type": null,
        "aggregate_ids": [],
    });

    let envelope = EventEnvelope::new(
        schema,
        "ReplayStarted",
        source,
        None,
        Uuid::new_v4(),
        payload,
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "replay_id": replay_id.to_string(),
                "status": "started",
                "from": from.to_rfc3339(),
                "to": to.map(|t| t.to_rfc3339()),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to start platform replay");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// OIDC Handlers
// ---------------------------------------------------------------------------

/// GET /.well-known/openid-configuration — OIDC discovery document
async fn handle_oidc_discovery(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    match iam.oidc_discovery().await {
        Ok(doc) => {
            let body = serde_json::to_value(&doc).unwrap_or_default();
            resource_response(body, ContentType::Json)
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /.well-known/jwks.json — JSON Web Key Set
async fn handle_jwks(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    match iam.jwks().await {
        Ok(jwks) => {
            let body = serde_json::to_value(&jwks).unwrap_or_default();
            resource_response(body, ContentType::Json)
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/token — Token endpoint (client_credentials grant)
#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
}

async fn handle_token(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<TokenRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    if req.grant_type != "client_credentials" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unsupported_grant_type",
                "error_description": "Only client_credentials grant is supported",
            })),
        )
            .into_response();
    }

    match iam
        .client_credentials_token(&req.client_id, &req.client_secret, req.scope.as_deref())
        .await
    {
        Ok(token_resp) => {
            let body = serde_json::to_value(&token_resp).unwrap_or_default();
            resource_response(body, ContentType::Json)
        }
        Err(picloud_domain::error::PiCloudError::Unauthenticated) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid_client",
                "error_description": "Invalid client_id or client_secret",
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /auth/authorize — Authorization endpoint.
///
/// Returns WebAuthn passkey authentication info. Browser clients use this
/// to discover the passkey registration/authentication endpoints.
async fn handle_authorize(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let issuer = format!("https://{}", state.cluster_domain.0);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "authentication_required",
            "method": "passkey",
            "message": "WebAuthn/FIDO2 passkey authentication is required.",
            "issuer": issuer,
            "register_begin": format!("{}/auth/register/begin", issuer),
            "register_complete": format!("{}/auth/register/complete", issuer),
            "login_begin": format!("{}/auth/login/begin", issuer),
            "login_complete": format!("{}/auth/login/complete", issuer),
            "enroll": format!("{}/auth/enroll", issuer),
            "device_begin": format!("{}/auth/device/begin", issuer),
            "device_poll": format!("{}/auth/device/poll", issuer),
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// WebAuthn / Passkey Handlers
// ---------------------------------------------------------------------------

/// POST /auth/register/begin — start passkey registration
#[derive(Deserialize)]
struct RegisterBeginRequest {
    /// The identity IRI to register a passkey for.
    identity_iri: String,
}

async fn handle_register_begin(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<RegisterBeginRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    let identity_iri = match ResourceIri::new(&req.identity_iri) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid IRI: {e}") })),
            )
                .into_response();
        }
    };

    match iam.begin_registration(&identity_iri).await {
        Ok((challenge_id, options)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "challenge_id": challenge_id,
                "options": serde_json::to_value(&options).unwrap_or_default(),
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/register/complete — complete passkey registration
#[derive(Deserialize)]
struct RegisterCompleteRequest {
    challenge_id: String,
    credential_id: String,
    public_key: String,
    attestation: Option<String>,
    aaguid: Option<String>,
    display_name: Option<String>,
}

async fn handle_register_complete(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<RegisterCompleteRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    let response = picloud_domain::identity::RegistrationResponse {
        credential_id: req.credential_id,
        public_key: req.public_key,
        attestation: req.attestation,
        aaguid: req.aaguid,
        display_name: req.display_name,
    };

    match iam.complete_registration(&req.challenge_id, response).await {
        Ok(passkey) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "registered",
                "credential_id": passkey.credential_id,
                "registered_at": passkey.registered_at.to_rfc3339(),
            })),
        )
            .into_response(),
        Err(picloud_domain::error::PiCloudError::PasskeyChallengeFailed { reason }) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/login/begin — start passkey authentication
#[derive(Deserialize)]
struct LoginBeginRequest {
    identity_iri: String,
}

async fn handle_login_begin(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<LoginBeginRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    let identity_iri = match ResourceIri::new(&req.identity_iri) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid IRI: {e}") })),
            )
                .into_response();
        }
    };

    match iam.begin_authentication(&identity_iri).await {
        Ok((challenge_id, options)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "challenge_id": challenge_id,
                "options": serde_json::to_value(&options).unwrap_or_default(),
            })),
        )
            .into_response(),
        Err(picloud_domain::error::PiCloudError::PasskeyChallengeFailed { reason }) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/login/complete — complete passkey authentication
#[derive(Deserialize)]
struct LoginCompleteRequest {
    challenge_id: String,
    credential_id: String,
    signature: String,
    authenticator_data: Option<String>,
    client_data_json: Option<String>,
    #[serde(default = "default_hmac_format")]
    signature_format: String,
}

fn default_hmac_format() -> String {
    "hmac".to_string()
}

async fn handle_login_complete(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<LoginCompleteRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    let response = picloud_domain::identity::AuthenticationResponse {
        credential_id: req.credential_id,
        signature: req.signature,
        authenticator_data: req.authenticator_data,
        client_data_json: req.client_data_json,
        signature_format: req.signature_format,
    };

    match iam.complete_authentication(&req.challenge_id, response).await {
        Ok(token) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "access_token": token,
                "token_type": "Bearer",
            })),
        )
            .into_response(),
        Err(picloud_domain::error::PiCloudError::PasskeyChallengeFailed { reason }) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/enroll — exchange an enrollment token for a registration challenge
#[derive(Deserialize)]
struct EnrollRequest {
    token: String,
}

async fn handle_enroll(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<EnrollRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    match iam.enroll_with_token(&req.token).await {
        Ok((challenge_id, options)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "challenge_id": challenge_id,
                "options": serde_json::to_value(&options).unwrap_or_default(),
            })),
        )
            .into_response(),
        Err(picloud_domain::error::PiCloudError::PasskeyChallengeFailed { reason }) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/device/begin — start a device flow (CLI uses this)
async fn handle_device_begin(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    match iam.begin_device_flow().await {
        Ok(flow) => {
            let body = serde_json::to_value(&flow).unwrap_or_default();
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /auth/device/poll — poll a device flow for completion
#[derive(Deserialize)]
struct DevicePollRequest {
    device_code: String,
}

async fn handle_device_poll(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<DevicePollRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    match iam.poll_device_flow(&req.device_code).await {
        Ok(result) => {
            let body = serde_json::to_value(&result).unwrap_or_default();
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(picloud_domain::error::PiCloudError::PasskeyChallengeFailed { reason }) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": reason })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Config store handlers (ADR-043)
// ---------------------------------------------------------------------------

/// GET /products/:name/config — list all config entries
async fn handle_config_list(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let config_iri = iri_builder.product_config(&name);

    // Query config entries from RDF graph
    let entries = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?key ?value ?type WHERE {{ ?entry <{PICLOUD_NS}configKey> ?key . ?entry <{PICLOUD_NS}configValue> ?value . ?entry <{PICLOUD_NS}configType> ?type . FILTER(STRSTARTS(STR(?entry), \"{}/\")) }}",
            config_iri.as_str(),
            PICLOUD_NS = "https://picloud.local/ontology#",
        );
        match projector.query(&sparql).await {
            Ok(result) => result
                .bindings
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "key": row.get("key").and_then(|v| v.get("value")),
                        "value": row.get("value").and_then(|v| v.get("value")),
                        "type": row.get("type").and_then(|v| v.get("value")),
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": config_iri.as_str(),
            "type": "ConfigStore",
            "product": name,
            "entries": entries,
        }),
        ct,
    )
}

/// GET /products/:name/config/:key — get a single config entry
async fn handle_config_get(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let entry_iri = iri_builder.config_entry(&name, &key);

    // Query specific config entry from RDF graph
    let entry = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?value ?type WHERE {{ <{}> <https://picloud.local/ontology#configValue> ?value . <{}> <https://picloud.local/ontology#configType> ?type }}",
            entry_iri.as_str(), entry_iri.as_str()
        );
        match projector.query(&sparql).await {
            Ok(result) => result.bindings.first().map(|row| {
                serde_json::json!({
                    "@id": entry_iri.as_str(),
                    "key": key,
                    "value": row.get("value").and_then(|v| v.get("value")),
                    "type": row.get("type").and_then(|v| v.get("value")),
                })
            }),
            Err(_) => None,
        }
    } else {
        None
    };

    match entry {
        Some(data) => resource_response(data, ct),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("config key '{}' not found", key) })),
        )
            .into_response(),
    }
}

/// Request payload for setting a config entry.
#[derive(Deserialize)]
struct ConfigSetPayload {
    key: String,
    value: String,
    #[serde(default = "default_config_type_str")]
    config_type: String,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

fn default_config_type_str() -> String {
    "string".to_string()
}

/// POST /products/:name/config — set a config entry (emits ConfigChanged)
async fn handle_config_set(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
    Json(payload): Json<ConfigSetPayload>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let config_iri = iri_builder.config_entry(&name, &payload.key);
    let correlation_id = Uuid::new_v4();

    let schema = iri_builder.event_schema("ConfigChanged", 1);
    let envelope = EventEnvelope::new(
        schema,
        "ConfigChanged",
        config_iri.clone(),
        Some(name.clone()),
        correlation_id,
        serde_json::json!({
            "config_iri": config_iri.as_str(),
            "product": name,
            "key": payload.key,
            "value": payload.value,
            "config_type": payload.config_type,
            "tags": payload.tags,
            "action": "set",
        }),
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "key": payload.key,
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append ConfigChanged event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// DELETE /products/:name/config/:key — delete a config entry
async fn handle_config_delete(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((name, key)): Path<(String, String)>,
) -> impl IntoResponse {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        );
    };

    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let config_iri = iri_builder.config_entry(&name, &key);
    let correlation_id = Uuid::new_v4();

    let schema = iri_builder.event_schema("ConfigChanged", 1);
    let envelope = EventEnvelope::new(
        schema,
        "ConfigChanged",
        config_iri.clone(),
        Some(name.clone()),
        correlation_id,
        serde_json::json!({
            "config_iri": config_iri.as_str(),
            "product": name,
            "key": key,
            "value": null,
            "action": "deleted",
        }),
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "key": key,
                "correlationId": correlation_id.to_string(),
            })),
        ),
        Err(e) => {
            warn!(error = %e, "Failed to append ConfigChanged (delete) event");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// GET /products/:name/containers/:cname/config — effective merged config
///
/// Returns the product-level config merged with any container-level overrides.
/// Container values win on key collision.
async fn handle_config_effective(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((name, cname)): Path<(String, String)>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let config_iri = iri_builder.product_config(&name);
    let container_iri = iri_builder.resource(&name, "containers", &cname);

    // In a full implementation we would:
    // 1. Load all product-level config entries
    // 2. Load container-level config overrides
    // 3. Merge with container values winning
    // For now, return product config with the container context
    let entries = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?key ?value ?type WHERE {{ ?entry <https://picloud.local/ontology#configKey> ?key . ?entry <https://picloud.local/ontology#configValue> ?value . ?entry <https://picloud.local/ontology#configType> ?type . FILTER(STRSTARTS(STR(?entry), \"{}/\")) }}",
            config_iri.as_str(),
        );
        match projector.query(&sparql).await {
            Ok(result) => result
                .bindings
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "key": row.get("key").and_then(|v| v.get("value")),
                        "value": row.get("value").and_then(|v| v.get("value")),
                        "type": row.get("type").and_then(|v| v.get("value")),
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": container_iri.as_str(),
            "type": "EffectiveConfig",
            "product": name,
            "container": cname,
            "entries": entries,
        }),
        ct,
    )
}

// ---------------------------------------------------------------------------
// Feature flag handlers (ADR-044)
// ---------------------------------------------------------------------------

/// GET /products/:name/flags — list all flags (evaluated against running version)
async fn handle_flags_list(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let flags_iri = iri_builder.product_flags(&name);

    // Get the product version from RDF to evaluate flags
    let product_version = get_product_version(&state, &name).await;

    // Query all flags from RDF graph
    let flags = if let Some(ref projector) = state.projector {
        let product_iri = iri_builder.product(&name);
        let sparql = format!(
            "SELECT ?flag ?fname ?enabled ?ver ?desc WHERE {{ ?flag <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://picloud.local/ontology#FeatureFlag> . ?flag <https://picloud.local/ontology#flagName> ?fname . ?flag <https://picloud.local/ontology#flagEnabled> ?enabled . ?flag <https://picloud.local/ontology#flagVersion> ?ver . OPTIONAL {{ ?flag <https://picloud.local/ontology#flagDescription> ?desc }} . FILTER(STRSTARTS(STR(?flag), \"{}/flags/\")) }}",
            product_iri.as_str()
        );
        match projector.query(&sparql).await {
            Ok(result) => result
                .bindings
                .iter()
                .map(|row| {
                    let flag_name = row.get("fname").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("");
                    let enabled_str = row.get("enabled").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("false");
                    let enabled = enabled_str == "true";
                    let version_expr = row.get("ver").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("");
                    let description = row.get("desc").and_then(|v| v.get("value")).and_then(|v| v.as_str());

                    let active = if enabled {
                        product_version.as_ref().map_or(false, |pv| {
                            picloud_domain::resources::parse_major_version(pv)
                                .and_then(|major| picloud_domain::resources::VersionOp::parse(version_expr).map(|op| op.matches(major)))
                                .unwrap_or(false)
                        })
                    } else {
                        false
                    };

                    serde_json::json!({
                        "name": flag_name,
                        "active": active,
                        "enabled": enabled,
                        "version": version_expr,
                        "description": description,
                    })
                })
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    resource_response(
        serde_json::json!({
            "@id": flags_iri.as_str(),
            "type": "FeatureFlagList",
            "product": name,
            "productVersion": product_version,
            "flags": flags,
        }),
        ct,
    )
}

/// GET /products/:name/flags/:flag — single flag evaluation
async fn handle_flag_get(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((name, flag_name)): Path<(String, String)>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let ct = content_type_from_headers(&headers);
    let flag_iri = iri_builder.feature_flag(&name, &flag_name);

    let product_version = get_product_version(&state, &name).await;

    let flag_data = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?enabled ?ver ?desc WHERE {{ <{}> <https://picloud.local/ontology#flagEnabled> ?enabled . <{}> <https://picloud.local/ontology#flagVersion> ?ver . OPTIONAL {{ <{}> <https://picloud.local/ontology#flagDescription> ?desc }} }}",
            flag_iri.as_str(), flag_iri.as_str(), flag_iri.as_str()
        );
        match projector.query(&sparql).await {
            Ok(result) => result.bindings.first().map(|row| {
                let enabled_str = row.get("enabled").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("false");
                let enabled = enabled_str == "true";
                let version_expr = row.get("ver").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("");
                let description = row.get("desc").and_then(|v| v.get("value")).and_then(|v| v.as_str());

                let active = if enabled {
                    product_version.as_ref().map_or(false, |pv| {
                        picloud_domain::resources::parse_major_version(pv)
                            .and_then(|major| picloud_domain::resources::VersionOp::parse(version_expr).map(|op| op.matches(major)))
                            .unwrap_or(false)
                    })
                } else {
                    false
                };

                serde_json::json!({
                    "@id": flag_iri.as_str(),
                    "name": flag_name,
                    "active": active,
                    "enabled": enabled,
                    "version": version_expr,
                    "description": description,
                })
            }),
            Err(_) => None,
        }
    } else {
        None
    };

    match flag_data {
        Some(data) => resource_response(data, ct),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("flag '{}' not found", flag_name) })),
        )
            .into_response(),
    }
}

/// GET /inference-rules — list all inference rules from the RDF graph (ADR-038)
async fn handle_inference_rules(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let ct = content_type_from_headers(&headers);
    let Some(ref projector) = state.projector else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "projector not available" })),
        )
            .into_response();
    };

    let sparql = "SELECT ?rule ?scope ?construct WHERE { \
        ?rule a <https://picloud.local/ontology#InferenceRule> . \
        OPTIONAL { ?rule <https://picloud.local/ontology#scope> ?scope } . \
        OPTIONAL { ?rule <https://picloud.local/ontology#constructQuery> ?construct } \
    }";

    match projector.query(sparql).await {
        Ok(result) => {
            let rules: Vec<serde_json::Value> = result
                .bindings
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "iri": b.get("rule").and_then(|v| v.as_str()).unwrap_or_default(),
                        "scope": b.get("scope").and_then(|v| v.as_str()).unwrap_or("platform"),
                    })
                })
                .collect();
            resource_response(serde_json::json!({ "inference_rules": rules }), ct)
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /groups — list all groups from the RDF graph (ADR-037)
async fn handle_groups(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let ct = content_type_from_headers(&headers);
    let Some(ref projector) = state.projector else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "projector not available" })),
        )
            .into_response();
    };

    let sparql = "SELECT ?group ?desc WHERE { \
        ?group a <https://picloud.local/ontology#Group> . \
        OPTIONAL { ?group <https://picloud.local/ontology#description> ?desc } \
    }";

    match projector.query(sparql).await {
        Ok(result) => {
            let groups: Vec<serde_json::Value> = result
                .bindings
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "iri": b.get("group").and_then(|v| v.as_str()).unwrap_or_default(),
                        "description": b.get("desc").and_then(|v| v.as_str()),
                    })
                })
                .collect();
            resource_response(serde_json::json!({ "groups": groups }), ct)
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /groups/:name — get a specific group with its roles and members (ADR-037)
async fn handle_group(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let ct = content_type_from_headers(&headers);
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let group_iri = iri_builder.group(&name);
    let Some(ref projector) = state.projector else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "projector not available" })),
        )
            .into_response();
    };

    // Get roles
    let roles_sparql = format!(
        "SELECT ?role WHERE {{ <{}> <https://picloud.local/ontology#hasRole> ?role }}",
        group_iri.as_str()
    );
    // Get members
    let members_sparql = format!(
        "SELECT ?member WHERE {{ <{}> <https://picloud.local/ontology#hasMember> ?member }}",
        group_iri.as_str()
    );
    // Get description
    let desc_sparql = format!(
        "SELECT ?desc WHERE {{ <{}> <https://picloud.local/ontology#description> ?desc }}",
        group_iri.as_str()
    );

    let roles = match projector.query(&roles_sparql).await {
        Ok(r) => r.bindings.iter()
            .filter_map(|b| b.get("role").and_then(|v| v.as_str()).map(String::from))
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    let members = match projector.query(&members_sparql).await {
        Ok(r) => r.bindings.iter()
            .filter_map(|b| b.get("member").and_then(|v| v.as_str()).map(String::from))
            .collect::<Vec<_>>(),
        Err(_) => vec![],
    };

    let description = match projector.query(&desc_sparql).await {
        Ok(r) => r.bindings.first()
            .and_then(|b| b.get("desc").and_then(|v| v.as_str()).map(String::from)),
        Err(_) => None,
    };

    if roles.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("group '{}' not found", name) })),
        )
            .into_response();
    }

    resource_response(
        serde_json::json!({
            "iri": group_iri.as_str(),
            "name": name,
            "description": description,
            "roles": roles,
            "members": members,
        }),
        ct,
    )
}

/// Helper to get a product's version from the RDF graph.
async fn get_product_version(state: &AppState, product_name: &str) -> Option<String> {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let product_iri = iri_builder.product(product_name);

    if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?version WHERE {{ <{}> <https://picloud.local/ontology#version> ?version }}",
            product_iri.as_str()
        );
        if let Ok(result) = projector.query(&sparql).await {
            return result.bindings.first()
                .and_then(|row| row.get("version"))
                .and_then(|v| v.get("value"))
                .and_then(|v| v.as_str())
                .map(String::from);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// OTel / Telemetry Handlers (ADR-045, ADR-046)
// ---------------------------------------------------------------------------

/// POST /otel — Accept OTLP JSON traces/metrics/logs and publish to OTel stream
async fn handle_otel_ingest(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(ref otel_stream) = state.otel_stream else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "otel_not_configured",
                "message": "OpenTelemetry stream is not configured"
            })),
        )
            .into_response();
    };

    let data = crate::otel::parse_otlp_json(&body);
    let count = data.len();

    for datum in data {
        otel_stream.publish(datum);
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "accepted": count,
        })),
    )
        .into_response()
}

/// Query parameters for telemetry queries.
#[derive(Deserialize)]
struct TelemetryQueryParams {
    /// Start time (RFC 3339)
    from: Option<String>,
    /// End time (RFC 3339)
    to: Option<String>,
    /// Filter by service name
    service: Option<String>,
    /// Filter by operation name (spans)
    operation: Option<String>,
    /// Filter by metric name (metrics)
    metric_name: Option<String>,
    /// Minimum duration in ms (spans)
    min_duration_ms: Option<u64>,
}

/// GET /telemetry/spans — Query stored spans
async fn handle_telemetry_spans(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(params): Query<TelemetryQueryParams>,
) -> Response {
    let Some(ref store) = state.telemetry_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "telemetry_not_configured",
                "message": "Telemetry store is not configured"
            })),
        )
            .into_response();
    };

    let now = chrono::Utc::now();
    let from = params
        .from
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| now - chrono::Duration::hours(1));
    let to = params
        .to
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or(now);

    let filter = picloud_domain::events::TelemetryFilter {
        service_name: params.service,
        operation_name: params.operation,
        metric_name: None,
        min_duration_ms: params.min_duration_ms,
    };

    match store.query_spans(from, to, filter).await {
        Ok(spans) => (StatusCode::OK, Json(serde_json::json!({ "spans": spans }))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "query_failed",
                "message": format!("{}", e),
            })),
        )
            .into_response(),
    }
}

/// GET /telemetry/metrics — Query stored metrics
async fn handle_telemetry_metrics(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(params): Query<TelemetryQueryParams>,
) -> Response {
    let Some(ref store) = state.telemetry_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "telemetry_not_configured",
                "message": "Telemetry store is not configured"
            })),
        )
            .into_response();
    };

    let now = chrono::Utc::now();
    let from = params
        .from
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|| now - chrono::Duration::hours(1));
    let to = params
        .to
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or(now);

    let filter = picloud_domain::events::TelemetryFilter {
        service_name: params.service,
        operation_name: None,
        metric_name: params.metric_name,
        min_duration_ms: None,
    };

    match store.query_metrics(from, to, filter).await {
        Ok(metrics) => {
            (StatusCode::OK, Json(serde_json::json!({ "metrics": metrics }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "query_failed",
                "message": format!("{}", e),
            })),
        )
            .into_response(),
    }
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
    async fn command_returns_unavailable_without_event_log() {
        let app = test_server().build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/api/commands")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"type":"ResourceDeclared","payload":{}}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Without event log injected, returns 503
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // -- OIDC endpoint tests --

    #[tokio::test]
    async fn oidc_discovery_returns_503_without_iam() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/.well-known/openid-configuration")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn jwks_returns_503_without_iam() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/.well-known/jwks.json")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn token_returns_503_without_iam() {
        let app = test_server().build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/auth/token")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"grant_type":"client_credentials","client_id":"x","client_secret":"y"}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn authorize_returns_passkey_required() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/auth/authorize")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "authentication_required");
        assert_eq!(json["method"], "passkey");
    }

    // -- Phase 3: Schema IRI serving tests --

    #[tokio::test]
    async fn event_schema_returns_json_schema() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/schemas/events/ResourceReady/v1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["$id"],
            "https://picloud.local/schemas/events/ResourceReady/v1"
        );
        assert_eq!(json["title"], "ResourceReady");
        assert_eq!(json["type"], "object");
        assert!(json["properties"]["event_type"].is_object());
    }

    #[tokio::test]
    async fn event_schema_invalid_version_returns_400() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/schemas/events/ResourceReady/vabc")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn product_event_schema_returns_json_schema() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/products/photo-app/schemas/events/OrderPlaced/v2")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["$id"],
            "https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v2"
        );
        assert_eq!(json["title"], "photo-app/OrderPlaced");
        assert_eq!(
            json["x-picloud-product"],
            "https://picloud.local/products/photo-app"
        );
    }

    #[tokio::test]
    async fn product_event_schema_invalid_version_returns_400() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/products/photo-app/schemas/events/OrderPlaced/vxyz")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // -- Phase 3: Ontology serving tests --

    #[tokio::test]
    async fn ontology_returns_stub_without_projector() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/products/photo-app/ontology")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["@id"],
            "https://picloud.local/products/photo-app/ontology"
        );
        assert_eq!(json["type"], "Ontology");
        assert_eq!(json["product"], "photo-app");
        assert_eq!(json["status"], "not_loaded");
    }

    // -- Phase 3: Cluster graph endpoint tests --

    #[tokio::test]
    async fn cluster_graph_returns_hint_without_query() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/graph")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["type"], "SparqlEndpoint");
        assert!(json["hint"].as_str().unwrap().contains("SPARQL"));
    }

    #[tokio::test]
    async fn cluster_graph_returns_error_without_projector() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/graph?query=SELECT%20*%20WHERE%20%7B%20%3Fs%20%3Fp%20%3Fo%20%7D")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["type"], "SparqlEndpoint");
        assert_eq!(json["error"], "projector not available");
    }

    // -- Product Event Store tests --

    fn test_server_with_event_log() -> PiCloudHttpServer {
        use picloud_events::InMemoryEventLog;
        let mut server = test_server();
        server.event_log = Some(Arc::new(InMemoryEventLog::new()));
        server
    }

    #[tokio::test]
    async fn event_store_append_returns_202() {
        let app = test_server_with_event_log().build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/products/photo-app/event-store/main/Order/order-123/events")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"schema":"https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v1","type":"OrderPlaced","payload":{"amount":42}}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "accepted");
        assert!(json["eventId"].is_string());
        assert!(json["correlationId"].is_string());
    }

    #[tokio::test]
    async fn event_store_append_invalid_schema_returns_400() {
        let app = test_server_with_event_log().build_router();
        let req = Request::builder()
            .method("POST")
            .uri("/products/photo-app/event-store/main/Order/order-123/events")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"schema":"not a valid iri","type":"OrderPlaced","payload":{}}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn event_store_read_returns_empty_initially() {
        let app = test_server_with_event_log().build_router();
        let req = Request::builder()
            .uri("/products/photo-app/event-store/main/Order/order-123/events")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["events"].as_array().unwrap().len(), 0);
        assert_eq!(json["product"], "photo-app");
        assert_eq!(json["store"], "main");
        assert_eq!(json["aggregateType"], "Order");
        assert_eq!(json["aggregateId"], "order-123");
    }

    #[tokio::test]
    async fn event_store_append_then_read_round_trip() {
        use picloud_events::InMemoryEventLog;
        let event_log = Arc::new(InMemoryEventLog::new());
        let mut server = test_server();
        server.event_log = Some(event_log.clone() as Arc<dyn EventLog>);
        let app = server.build_router();

        // Append an event
        let req = Request::builder()
            .method("POST")
            .uri("/products/photo-app/event-store/main/Order/order-42/events")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"schema":"https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v1","type":"OrderPlaced","payload":{"amount":100}}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // Read events back
        let req = Request::builder()
            .uri("/products/photo-app/event-store/main/Order/order-42/events")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "OrderPlaced");
        assert_eq!(events[0]["payload"]["amount"], 100);
    }

    #[tokio::test]
    async fn event_store_read_filters_by_aggregate() {
        use picloud_events::InMemoryEventLog;
        let event_log = Arc::new(InMemoryEventLog::new());
        let mut server = test_server();
        server.event_log = Some(event_log.clone() as Arc<dyn EventLog>);
        let app = server.build_router();

        // Append event to order-1
        let req = Request::builder()
            .method("POST")
            .uri("/products/photo-app/event-store/main/Order/order-1/events")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"schema":"https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v1","type":"OrderPlaced","payload":{"id":"order-1"}}"#,
            ))
            .unwrap();
        app.clone().oneshot(req).await.unwrap();

        // Append event to order-2
        let req = Request::builder()
            .method("POST")
            .uri("/products/photo-app/event-store/main/Order/order-2/events")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"schema":"https://picloud.local/products/photo-app/schemas/events/OrderPlaced/v1","type":"OrderPlaced","payload":{"id":"order-2"}}"#,
            ))
            .unwrap();
        app.clone().oneshot(req).await.unwrap();

        // Read only order-1 events
        let req = Request::builder()
            .uri("/products/photo-app/event-store/main/Order/order-1/events")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["payload"]["id"], "order-1");

        // Read only order-2 events
        let req = Request::builder()
            .uri("/products/photo-app/event-store/main/Order/order-2/events")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let events = json["events"].as_array().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["payload"]["id"], "order-2");
    }

    #[tokio::test]
    async fn event_store_returns_503_without_event_log() {
        let app = test_server().build_router();

        // POST
        let req = Request::builder()
            .method("POST")
            .uri("/products/photo-app/event-store/main/Order/order-1/events")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"schema":"https://picloud.local/schemas/events/X/v1","type":"X","payload":{}}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        // GET
        let req = Request::builder()
            .uri("/products/photo-app/event-store/main/Order/order-1/events")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
