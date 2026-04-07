/// HTTP Server Implementation
///
/// Provides the axum-based HTTP server, IRI routing, and content negotiation
/// for the PiCloud platform. Every resource has a dereferenceable IRI that
/// maps directly to an HTTP route.
///
/// This crate depends only on picloud-domain. Trait objects for event log,
/// state projection, etc. are injected from the composition root.
use std::net::SocketAddr;
use std::sync::Arc;

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
use picloud_domain::events::EventEnvelope;
use picloud_domain::iri::{ClusterDomain, IriBuilder, ResourceIri};
use picloud_domain::parser::{ResourceDeclaration, ResourceFile};
use picloud_domain::traits::{
    ClusterMembership, EventFilter, EventLog, IdentityProvider, StateProjector,
    StorageBackend, WorkloadScheduler,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
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
            .route("/api/apply", post(handle_apply))
            .route("/api/delete", post(handle_delete))
            .route("/graph", get(handle_cluster_graph))
            .route(
                "/.well-known/openid-configuration",
                get(handle_oidc_discovery),
            )
            .route("/.well-known/jwks.json", get(handle_jwks))
            .route("/auth/token", post(handle_token))
            .route("/auth/authorize", get(handle_authorize))
            .with_state(AppState {
                cluster_root_iri,
                cluster_domain: self.cluster_domain.clone(),
                event_log: self.event_log.clone(),
                projector: self.projector.clone(),
                cluster: self.cluster.clone(),
                iam: self.iam.clone(),
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
    event_log: Option<Arc<dyn EventLog>>,
    projector: Option<Arc<dyn StateProjector>>,
    cluster: Option<Arc<dyn ClusterMembership>>,
    iam: Option<Arc<dyn IdentityProvider>>,
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

/// GET /auth/authorize — Authorization endpoint stub (Phase 2)
///
/// Full WebAuthn/passkey ceremony is Phase 3. For now, return a JSON
/// response indicating that passkey authentication is required.
async fn handle_authorize(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let issuer = format!("https://{}", state.cluster_domain.0);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "authentication_required",
            "method": "passkey",
            "message": "WebAuthn/FIDO2 passkey authentication is required. Full ceremony available in Phase 3.",
            "issuer": issuer,
            "authorize_endpoint": format!("{}/auth/authorize", issuer),
        })),
    )
        .into_response()
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
}
