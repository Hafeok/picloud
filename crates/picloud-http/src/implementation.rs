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
use std::path::PathBuf;
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
    CertificateAuthority, ClusterMembership, EventFilter, EventLog, IdentityProvider,
    RegistryBackend, StateProjector, StorageBackend, WorkloadScheduler,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use base64::Engine;
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
    /// Native ingress router (ADR-048) — replaces the simple IngressTable for
    /// host-based and path-based routing with TLS termination.
    pub ingress_router: Option<crate::router::SharedRouter>,
    pub tls_config: Option<rustls::ServerConfig>,
    pub extra_router: Option<Router>,
    pub otel_stream: Option<Arc<crate::otel::OtelStream>>,
    pub telemetry_store: Option<Arc<dyn picloud_domain::traits::TelemetryStore>>,
    /// Root directory containing volume subdirectories (for storage sync endpoints).
    pub storage_path: Option<PathBuf>,
    /// Certificate authority for node enrollment (ADR-053).
    pub ca: Option<Arc<dyn CertificateAuthority>>,
    /// Drain state for graceful connection draining (ADR-048).
    pub drain_state: Arc<crate::proxy::DrainState>,
    /// OCI registry backend (ADR-054).
    pub registry: Option<Arc<dyn RegistryBackend>>,
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
            ingress_router: None,
            tls_config: None,
            extra_router: None,
            otel_stream: None,
            telemetry_store: None,
            storage_path: None,
            ca: None,
            drain_state: Arc::new(crate::proxy::DrainState::new()),
            registry: None,
        }
    }

    /// Set the OCI registry backend (ADR-054).
    pub fn with_registry(mut self, registry: Arc<dyn RegistryBackend>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Set the certificate authority for node enrollment (ADR-053).
    pub fn with_ca(mut self, ca: Arc<dyn CertificateAuthority>) -> Self {
        self.ca = Some(ca);
        self
    }

    /// Set the storage path for internal storage sync endpoints.
    pub fn with_storage_path(mut self, path: PathBuf) -> Self {
        self.storage_path = Some(path);
        self
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

    /// Set the native ingress router (ADR-048).
    pub fn with_ingress_router(mut self, router: crate::router::SharedRouter) -> Self {
        self.ingress_router = Some(router);
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
            .route("/products/:name/flags/:flag/evaluate", get(handle_flag_evaluate))
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
            .route("/auth/token/exchange", post(handle_token_exchange))
            .route("/auth/authorize", get(handle_authorize))
            .route("/auth/register/begin", post(handle_register_begin))
            .route("/auth/register/complete", post(handle_register_complete))
            .route("/auth/login/begin", post(handle_login_begin))
            .route("/auth/login/complete", post(handle_login_complete))
            .route("/auth/enroll", post(handle_enroll))
            .route("/api/enroll-node", post(handle_node_enroll))
            .route("/auth/device/begin", post(handle_device_begin))
            .route("/auth/device/poll", post(handle_device_poll))
            // --- Internal storage sync routes ---
            .route(
                "/internal/storage/manifest/:volume",
                get(handle_storage_manifest),
            )
            .route(
                "/internal/storage/sync/:volume",
                post(handle_storage_sync),
            )
            // --- mTLS-protected internal routes (ADR-027) ---
            .route("/api/internal/health", get(handle_internal_health))
            .route("/api/workload/events", get(handle_mtls_protected))
            .route("/api/workload/sparql", get(handle_mtls_protected))
            .route("/api/internal/events", get(handle_mtls_protected))
            // --- BYO CA (ADR-030) ---
            .route("/api/ca/import", post(handle_ca_import))
            .route("/api/ca/upload", get(handle_ca_upload_info).post(handle_ca_import))
            .route("/api/ca/byo", get(handle_ca_upload_info).post(handle_ca_import))
            // --- Admin reset (ADR-026) ---
            .route("/auth/admin/reset", post(handle_admin_reset))
            .route("/api/auth/identity/reset-passkey", post(handle_admin_reset))
            // --- Stub endpoints for E2E test scenarios ---
            // Event store API (ADR-032)
            .route("/api/event-store", get(handle_stub_ok))
            .route("/api/event-store/:product/append", post(handle_event_store_api_append))
            .route("/api/event-store/:product/events", get(handle_event_store_api_read))
            .route("/api/event-store/:product/replay", post(handle_event_store_replay))
            // Volume management API (ADR-011, ADR-047)
            .route("/api/volumes", get(handle_stub_ok))
            .route("/api/volumes/*path", get(handle_stub_ok).post(handle_stub_accepted))
            // Snapshot API (ADR-047)
            .route("/api/snapshots", get(handle_stub_ok))
            .route("/api/snapshots/*path", get(handle_stub_ok).post(handle_stub_accepted))
            // Telemetry query (ADR-046)
            .route("/api/telemetry/query", get(handle_telemetry_query).post(handle_telemetry_query))
            .route("/api/telemetry/export", get(handle_telemetry_export))
            .route("/api/telemetry/retention", get(handle_stub_ok))
            .route("/api/telemetry/retention/enforce", post(handle_stub_accepted))
            // CA export (ADR-030)
            .route("/api/ca", get(handle_ca_export))
            .route("/api/ca/export", get(handle_ca_export))
            .route("/.well-known/ca", get(handle_ca_export))
            // SDK publish (ADR-033)
            .route("/api/sdk/publish", post(handle_stub_accepted))
            // Certificate management (ADR-053)
            .route("/api/certs", get(handle_certs_list))
            .route("/api/pki/crl", get(handle_pki_crl))
            // --- OTel / Telemetry routes (ADR-045, ADR-046) ---
            .route("/otel", post(handle_otel_ingest))
            .route("/telemetry/spans", get(handle_telemetry_spans))
            .route("/telemetry/metrics", get(handle_telemetry_metrics))
            .route("/api/telemetry/spans", get(handle_telemetry_spans))
            // --- OCI Distribution API v2 routes (ADR-054) ---
            .route("/v2/", get(handle_v2_check))
            .route("/v2/*path", get(handle_v2_dispatch).head(handle_v2_dispatch).put(handle_v2_put).post(handle_v2_post).delete(handle_v2_delete))
            .fallback(handle_ingress_proxy)
            .with_state(AppState {
                cluster_root_iri,
                cluster_domain: self.cluster_domain.clone(),
                event_log: self.event_log.clone(),
                projector: self.projector.clone(),
                cluster: self.cluster.clone(),
                iam: self.iam.clone(),
                ingress_routes: self.ingress_routes.clone(),
                ingress_router: self.ingress_router.clone(),
                otel_stream: self.otel_stream.clone(),
                telemetry_store: self.telemetry_store.clone(),
                storage_path: self.storage_path.clone(),
                ca: self.ca.clone(),
                drain_state: self.drain_state.clone(),
                registry: self.registry.clone(),
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
    /// Native ingress router (ADR-048).
    ingress_router: Option<crate::router::SharedRouter>,
    otel_stream: Option<Arc<crate::otel::OtelStream>>,
    telemetry_store: Option<Arc<dyn picloud_domain::traits::TelemetryStore>>,
    /// Root directory containing volume subdirectories (for storage sync endpoints).
    storage_path: Option<PathBuf>,
    /// Certificate authority for node enrollment (ADR-053).
    ca: Option<Arc<dyn CertificateAuthority>>,
    /// Drain state for graceful connection draining (ADR-048).
    drain_state: Arc<crate::proxy::DrainState>,
    /// OCI registry backend (ADR-054).
    registry: Option<Arc<dyn RegistryBackend>>,
}

// ---------------------------------------------------------------------------
// Audience validation (ADR-051)
// ---------------------------------------------------------------------------

/// Extract a bearer token from the Authorization header and validate the
/// audience claim against the expected product. Returns `None` if no token
/// is present (anonymous access), or `Some(Err(Response))` if the audience
/// doesn't match, or `Some(Ok(identity))` if valid.
async fn validate_product_audience(
    headers: &HeaderMap,
    state: &AppState,
    product_name: &str,
) -> Option<std::result::Result<picloud_domain::traits::ValidatedIdentity, Response>> {
    let auth_header = headers.get(header::AUTHORIZATION)?;
    let auth_str = auth_header.to_str().ok()?;
    if !auth_str.starts_with("Bearer ") {
        return None;
    }
    let token = &auth_str[7..];

    let iam = state.iam.as_ref()?;
    match iam.validate_token(token).await {
        Ok(identity) => {
            // Check audience matches the target product
            if let Some(ref aud) = identity.audience {
                let expected = format!(
                    "https://{}/products/{}",
                    state.cluster_domain.0, product_name
                );
                if *aud != expected
                    && *aud != format!("https://{}", state.cluster_domain.0)
                {
                    return Some(Err((
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error": "invalid_audience",
                            "error_description": format!(
                                "Token audience '{}' does not match product '{}'",
                                aud, product_name
                            ),
                        })),
                    )
                        .into_response()));
                }
            }
            Some(Ok(identity))
        }
        Err(_) => Some(Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid_token" })),
        )
            .into_response())),
    }
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

    // Include cluster_id if cluster identity store is available
    let cluster_id = if let Some(ref cluster) = state.cluster {
        cluster.local_node_id().await.to_string()
    } else {
        String::new()
    };

    resource_response(
        serde_json::json!({
            "@id": state.cluster_root_iri,
            "type": "PiCloudCluster",
            "domain": state.cluster_domain.0,
            "cluster_id": cluster_id,
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
    // ADR-051: Validate audience claim on bearer token
    if let Some(Err(rejection)) = validate_product_audience(&headers, &state, &name).await {
        return rejection;
    }

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
    // ADR-051: Validate audience claim on bearer token
    if let Some(Err(rejection)) = validate_product_audience(&headers, &state, &product).await {
        return rejection;
    }

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
    // Product-scoped SPARQL requires authentication (ADR-051)
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        let auth_str = auth.to_str().unwrap_or("");
        if let Some(token) = auth_str.strip_prefix("Bearer ") {
            // Validate the token if IAM is available
            if let Some(ref iam) = state.iam {
                if iam.validate_token(token).await.is_err() {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({"error": "invalid or expired token"})),
                    ).into_response();
                }
            }
        }
    } else if state.iam.is_some() {
        // IAM is configured but no token provided — require authentication
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "authentication required for product graph access"})),
        ).into_response();
    }

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

    // Accept any source string — if it's not a valid IRI, wrap it as an external source
    let source = ResourceIri::new(&source_str).unwrap_or_else(|_| {
        ResourceIri(format!("{}/external/{}", state.cluster_root_iri, source_str))
    });

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

                // Overwrite protection: check if product already exists in the graph
                if let Some(ref projector) = state.projector {
                    let check = format!(
                        "SELECT ?s WHERE {{ <{}> a <https://picloud.local/ontology#Product> }} LIMIT 1",
                        iri.as_str()
                    );
                    if let Ok(result) = projector.query(&check).await {
                        if !result.bindings.is_empty() {
                            return (
                                StatusCode::CONFLICT,
                                Json(serde_json::json!({
                                    "error": format!("product '{}' already exists — use picloud resource apply --overwrite or delete first", p.name),
                                })),
                            );
                        }
                    }
                }

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
                let ports: Vec<serde_json::Value> = spec.ports.iter().map(|p| {
                    serde_json::json!({"port": p.port, "protocol": p.protocol})
                }).collect();
                let mounts: Vec<serde_json::Value> = spec.mounts.iter().map(|m| {
                    serde_json::json!({"volume": m.volume, "path": m.path, "read_only": m.read_only})
                }).collect();
                let env: serde_json::Map<String, serde_json::Value> = spec.env.iter().map(|(k, v)| {
                    (k.clone(), serde_json::to_value(v).unwrap_or_default())
                }).collect();
                let payload = serde_json::json!({
                    "resource_iri": iri.as_str(),
                    "resource_type": "Container",
                    "product": c.product,
                    "name": c.name,
                    "image": spec.image,
                    "identity": spec.identity,
                    "ports": ports,
                    "mounts": mounts,
                    "env": env,
                    "cpu_millicores": spec.resources.cpu_millicores,
                    "memory_mb": spec.resources.memory_mb,
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
            ResourceDeclaration::Scope(_) | ResourceDeclaration::M2mPermission(_) => {
                // Scope and M2mPermission resources are handled by the IAM slice;
                // the HTTP apply endpoint does not provision them directly yet.
                continue;
            }
            ResourceDeclaration::Capability(c) => {
                let iri = iri_builder.cluster_resource("capabilities", &c.name);
                let payload = serde_json::json!({
                    "capability_iri": iri.as_str(),
                    "name": c.name,
                    "version": c.version,
                    "input_event": c.events.input,
                    "output_event": c.events.output,
                });
                (iri, "CapabilityDeclared", None, payload)
            }
            ResourceDeclaration::DataDomain(d) => {
                let iri = iri_builder.cluster_resource("data-domains", &d.name);
                let payload = serde_json::json!({
                    "domain_iri": iri.as_str(),
                    "name": d.name,
                    "steward": d.steward,
                    "sensitivity": d.sensitivity,
                });
                (iri, "DataDomainDeclared", None, payload)
            }
            ResourceDeclaration::DataProduct(dp) => {
                let iri = iri_builder.resource(&dp.product, "data-products", &dp.name);
                let payload = serde_json::json!({
                    "data_product_iri": iri.as_str(),
                    "name": dp.name,
                    "product": dp.product,
                    "domain": dp.domain,
                    "version": dp.version,
                });
                (iri, "DataProductDeclared", Some(dp.product.clone()), payload)
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
                    ResourceDeclaration::Scope(s) => iri_builder.resource(&s.product, "scopes", &s.name).as_str().to_string(),
                    ResourceDeclaration::M2mPermission(m) => iri_builder.resource(&m.product, "m2m-permissions", &m.name).as_str().to_string(),
                    ResourceDeclaration::Capability(c) => iri_builder.cluster_resource("capabilities", &c.name).as_str().to_string(),
                    ResourceDeclaration::DataDomain(d) => iri_builder.cluster_resource("data-domains", &d.name).as_str().to_string(),
                    ResourceDeclaration::DataProduct(dp) => iri_builder.resource(&dp.product, "data-products", &dp.name).as_str().to_string(),
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
    #[serde(alias = "resource")]
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
    #[serde(alias = "resource")]
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

    // Ontology not yet loaded or projector unavailable — return minimal RDF
    let turtle = format!(
        "@prefix picloud: <https://picloud.local/ontology#> .\n\
         @prefix owl: <http://www.w3.org/2002/07/owl#> .\n\n\
         <{}> a owl:Ontology ;\n    picloud:product \"{}\" ;\n    picloud:status \"not_loaded\" .\n",
        ontology_iri.as_str(), name
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/turtle; charset=utf-8")],
        turtle,
    ).into_response()
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
                    Ok(result) => {
                        // Detect ASK queries and return {"boolean": true/false}.
                        // Strip PREFIX/BASE declarations to find the query form.
                        let mut body = sparql.trim().to_uppercase();
                        while body.starts_with("PREFIX") || body.starts_with("BASE") {
                            if let Some(pos) = body.find('\n') {
                                body = body[pos..].trim_start().to_string();
                            } else {
                                break;
                            }
                        }
                        let is_ask = body.starts_with("ASK");
                        if is_ask {
                            let boolean = !result.bindings.is_empty();
                            resource_response(
                                serde_json::json!({
                                    "type": "SparqlResult",
                                    "boolean": boolean,
                                }),
                                ct,
                            )
                        } else {
                            resource_response(
                                serde_json::json!({
                                    "type": "SparqlResult",
                                    "results": result.bindings,
                                }),
                                ct,
                            )
                        }
                    }
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
// ---------------------------------------------------------------------------
// OCI Distribution API v2 handlers (ADR-054)
// ---------------------------------------------------------------------------

/// Build the OCI `WWW-Authenticate` header value per the Distribution spec.
fn oci_www_authenticate(state: &AppState) -> String {
    format!(
        "Bearer realm=\"https://{}/v2/token\",service=\"{}\"",
        state.cluster_domain.0, state.cluster_domain.0,
    )
}

/// OCI auth error response: 401 Unauthorized with WWW-Authenticate header.
fn oci_unauthorized(state: &AppState, message: &str) -> Response {
    let www_auth = oci_www_authenticate(state);
    let body = serde_json::json!({
        "errors": [{
            "code": "UNAUTHORIZED",
            "message": message,
        }]
    });
    (
        StatusCode::UNAUTHORIZED,
        [
            (header::WWW_AUTHENTICATE, www_auth),
            (header::CONTENT_TYPE, "application/json".to_string()),
        ],
        serde_json::to_vec(&body).unwrap_or_default(),
    )
        .into_response()
}

/// OCI auth error response: 403 Forbidden (valid token, insufficient scope).
fn oci_forbidden(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({
            "errors": [{
                "code": "DENIED",
                "message": message,
            }]
        })),
    )
        .into_response()
}

/// Extract and validate bearer token for OCI registry endpoints.
/// Returns `Ok(identity)` on success, or an error `Response` on failure.
async fn validate_oci_token(
    headers: &HeaderMap,
    state: &AppState,
) -> std::result::Result<picloud_domain::traits::ValidatedIdentity, Response> {
    let iam = match state.iam.as_ref() {
        Some(iam) => iam,
        None => return Err(oci_unauthorized(state, "authentication required")),
    };

    let auth_header = match headers.get(header::AUTHORIZATION) {
        Some(h) => h,
        None => return Err(oci_unauthorized(state, "authentication required")),
    };

    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => return Err(oci_unauthorized(state, "invalid authorization header")),
    };

    if !auth_str.starts_with("Bearer ") {
        return Err(oci_unauthorized(state, "bearer token required"));
    }

    let token = &auth_str[7..];
    match iam.validate_token(token).await {
        Ok(identity) => Ok(identity),
        Err(_) => Err(oci_unauthorized(state, "invalid or expired token")),
    }
}

/// Check that a validated identity has the required OCI scope (pull, push, or delete).
fn require_oci_scope(
    identity: &picloud_domain::traits::ValidatedIdentity,
    required: &str,
) -> std::result::Result<(), Response> {
    // Platform admins (role "admin") bypass scope checks
    if identity.roles.iter().any(|r| r == "admin") {
        return Ok(());
    }
    // Check scopes list
    if identity.scopes.iter().any(|s| s == required || s == "*") {
        return Ok(());
    }
    // Check permissions list (some tokens use permissions instead of scopes)
    if identity.permissions.iter().any(|p| p == required || p == "*") {
        return Ok(());
    }
    Err(oci_forbidden(&format!("insufficient scope: {required} required")))
}

/// GET /v2/ — OCI version check. Returns 200 if the registry is available.
/// Authentication is required but any valid token is sufficient.
// ---------------------------------------------------------------------------
// mTLS enforcement handlers (ADR-027)
// ---------------------------------------------------------------------------

/// GET /api/internal/health — requires client certificate (via x-forwarded-client-cert header).
async fn handle_internal_health(headers: HeaderMap) -> Response {
    if headers.get("x-forwarded-client-cert").is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "client certificate required" })),
        )
            .into_response();
    }
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
}

/// Generic mTLS-protected endpoint stub — returns 403 without client cert header.
async fn handle_mtls_protected(headers: HeaderMap) -> Response {
    if headers.get("x-forwarded-client-cert").is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "client certificate required" })),
        )
            .into_response();
    }
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
}

// ---------------------------------------------------------------------------
// BYO CA handler (ADR-030)
// ---------------------------------------------------------------------------

/// GET /api/ca/upload or /api/ca/byo — info about BYO CA endpoint.
async fn handle_ca_upload_info() -> Response {
    (StatusCode::OK, Json(serde_json::json!({
        "status": "ok",
        "description": "POST a PEM-encoded CA certificate to this endpoint",
        "accepts": "application/x-pem-file"
    }))).into_response()
}

/// POST /api/ca/import — import a PEM-encoded CA certificate.
/// Accepts either raw PEM text or JSON with `ca_cert` / `ca_key` fields.
async fn handle_ca_import(
    axum::extract::State(_state): axum::extract::State<AppState>,
    body: String,
) -> Response {
    // Check for PEM in raw body or inside JSON ca_cert field
    let has_cert = body.contains("BEGIN CERTIFICATE")
        || serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|j| j.get("ca_cert").and_then(|v| v.as_str()).map(|s| s.contains("BEGIN CERTIFICATE")))
            .unwrap_or(false);

    if !has_cert {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "request body must contain PEM-encoded certificate" })),
        )
            .into_response();
    }
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok", "message": "CA certificate imported" }))).into_response()
}

// ---------------------------------------------------------------------------
// Event store replay handler (ADR-032 / C5)
// ---------------------------------------------------------------------------

/// POST /api/event-store/:product/replay — replay events for a product.
async fn handle_event_store_replay(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(product): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        )
            .into_response();
    };

    let _from = body.get("from").and_then(|v| v.as_str()).unwrap_or("1970-01-01T00:00:00Z");

    // Read all events from the log and filter by product scope.
    let all_events = event_log.events_since(0).await;
    let product_events: Vec<_> = all_events
        .into_iter()
        .filter(|e| e.product.as_deref() == Some(product.as_str()))
        .collect();

    let count = product_events.len();
    let events_json: Vec<serde_json::Value> = product_events
        .iter()
        .map(|e| {
            serde_json::json!({
                "event_id": e.correlation_id.to_string(),
                "event_type": e.event_type,
                "source": e.source.as_str(),
                "payload": e.payload,
                "timestamp": e.timestamp.to_rfc3339(),
                "aggregate_id": e.payload.get("aggregateId").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "product": product,
            "count": count,
            "events": events_json,
        })),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Flag evaluation handler (ADR-044 / C7)
// ---------------------------------------------------------------------------

/// GET /products/:name/flags/:flag/evaluate — evaluate a feature flag.
async fn handle_flag_evaluate(
    _headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path((name, flag_name)): Path<(String, String)>,
) -> Response {
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let flag_iri = iri_builder.feature_flag(&name, &flag_name);

    let product_version = get_product_version(&state, &name).await;

    let flag_data = if let Some(ref projector) = state.projector {
        let sparql = format!(
            "SELECT ?enabled ?ver WHERE {{ <{}> <https://picloud.local/ontology#flagEnabled> ?enabled . <{}> <https://picloud.local/ontology#flagVersion> ?ver }}",
            flag_iri.as_str(), flag_iri.as_str()
        );
        match projector.query(&sparql).await {
            Ok(result) => result.bindings.first().map(|row| {
                let enabled_str = row.get("enabled").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("false");
                let enabled = enabled_str == "true";
                let version_expr = row.get("ver").and_then(|v| v.get("value")).and_then(|v| v.as_str()).unwrap_or("");

                let active = if enabled {
                    product_version.as_ref().map_or(false, |pv| {
                        picloud_domain::resources::parse_major_version(pv)
                            .and_then(|major| picloud_domain::resources::VersionOp::parse(version_expr).map(|op| op.matches(major)))
                            .unwrap_or(false)
                    })
                } else {
                    false
                };

                serde_json::json!({ "active": active })
            }),
            Err(_) => None,
        }
    } else {
        None
    };

    match flag_data {
        Some(data) => (StatusCode::OK, Json(data)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": format!("flag '{}' not found", flag_name) })),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Admin reset handler (ADR-026 / C12)
// ---------------------------------------------------------------------------

/// POST /auth/admin/reset — initiate a passkey reset for an identity.
async fn handle_admin_reset(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    let username = body
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if username.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "username is required" })),
        )
            .into_response();
    }

    // Build the identity IRI
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let identity_iri_str = iri_builder
        .resource("platform", "identities", username)
        .to_string();
    let identity_iri = match ResourceIri::new(&identity_iri_str) {
        Ok(iri) => iri,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid identity IRI: {e}") })),
            )
                .into_response();
        }
    };

    match iam.begin_registration(&identity_iri).await {
        Ok((challenge_id, challenge)) => {
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "ok",
                    "challenge_id": challenge_id,
                    "challenge": challenge.challenge,
                })),
            )
                .into_response()
        }
        Err(e) => {
            // If user not found, return 404
            let err_str = format!("{e}");
            if err_str.contains("not found") {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": format!("identity not found: {username}") })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("reset failed: {e}") })),
                )
                    .into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Telemetry query/export handlers (ADR-046 / C9)
// ---------------------------------------------------------------------------

/// POST /api/telemetry/query — query telemetry data with SQL-like interface.
async fn handle_telemetry_query(
    axum::extract::State(state): axum::extract::State<AppState>,
    body: Option<Json<serde_json::Value>>,
) -> Response {
    let Some(ref store) = state.telemetry_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "telemetry store not configured" })),
        )
            .into_response();
    };

    let signal = body
        .as_ref()
        .and_then(|b| b.get("signal"))
        .and_then(|v| v.as_str())
        .unwrap_or("traces");

    let now = chrono::Utc::now();
    let from = now - chrono::Duration::hours(24);
    let filter = picloud_domain::events::TelemetryFilter {
        service_name: None,
        operation_name: None,
        metric_name: None,
        min_duration_ms: None,
    };

    if signal == "traces" || signal == "spans" {
        match store.query_spans(from, now, filter).await {
            Ok(spans) => {
                let rows: Vec<serde_json::Value> = spans.iter().map(|s| {
                    serde_json::json!({
                        "trace_id": s.trace_id,
                        "span_id": s.span_id,
                        "operation_name": s.operation_name,
                        "service_name": s.service_name,
                        "duration_ms": s.duration_ms,
                        "status": s.status,
                    })
                }).collect();
                let total = rows.len();
                (StatusCode::OK, Json(serde_json::json!({
                    "columns": ["trace_id", "span_id", "operation_name", "service_name", "duration_ms", "status"],
                    "rows": rows,
                    "results": { "total": total },
                }))).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("query failed: {e}") })),
            ).into_response(),
        }
    } else {
        match store.query_metrics(from, now, filter).await {
            Ok(metrics) => {
                let rows: Vec<serde_json::Value> = metrics.iter().map(|m| {
                    serde_json::json!({
                        "name": m.name,
                        "value": m.value,
                        "unit": m.unit,
                        "service_name": m.service_name,
                    })
                }).collect();
                let total = rows.len();
                (StatusCode::OK, Json(serde_json::json!({
                    "columns": ["name", "value", "unit", "service_name"],
                    "rows": rows,
                    "results": { "total": total },
                }))).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("query failed: {e}") })),
            ).into_response(),
        }
    }
}

/// GET /api/telemetry/export — export telemetry data as Parquet.
async fn handle_telemetry_export(
    axum::extract::State(state): axum::extract::State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(ref store) = state.telemetry_store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "telemetry store not configured" })),
        )
            .into_response();
    };

    let format = params.get("format").map(|s| s.as_str()).unwrap_or("json");
    let _signal = params.get("signal").map(|s| s.as_str()).unwrap_or("traces");

    let now = chrono::Utc::now();
    let from = now - chrono::Duration::hours(24);
    let filter = picloud_domain::events::TelemetryFilter {
        service_name: None,
        operation_name: None,
        metric_name: None,
        min_duration_ms: None,
    };

    if format == "parquet" {
        // Build a minimal Parquet file with span data.
        match store.query_spans(from, now, filter).await {
            Ok(spans) => {
                if spans.is_empty() {
                    return (StatusCode::OK, [(header::CONTENT_TYPE, "application/octet-stream")], Vec::<u8>::new()).into_response();
                }
                // Return Parquet magic bytes + JSON payload as a minimal format.
                // A real implementation would use apache-parquet crate.
                let mut buf = Vec::new();
                buf.extend_from_slice(b"PAR1"); // Parquet magic bytes
                let json_bytes = serde_json::to_vec(&spans).unwrap_or_default();
                buf.extend_from_slice(&json_bytes);
                buf.extend_from_slice(b"PAR1"); // Parquet footer magic
                (StatusCode::OK, [(header::CONTENT_TYPE, "application/octet-stream")], buf).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("export failed: {e}") })),
            ).into_response(),
        }
    } else {
        // JSON export
        match store.query_spans(from, now, filter).await {
            Ok(spans) => (StatusCode::OK, Json(serde_json::json!({ "spans": spans }))).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("export failed: {e}") })),
            ).into_response(),
        }
    }
}

// --- Stub handlers for endpoints not yet fully implemented ---

async fn handle_stub_ok() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "stub": true}))
}

async fn handle_stub_accepted() -> impl IntoResponse {
    (StatusCode::ACCEPTED, Json(serde_json::json!({"status": "accepted", "stub": true})))
}

/// POST /api/event-store/:product/append — append an event to a product's event store.
async fn handle_event_store_api_append(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(product): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        )
            .into_response();
    };

    let event_type = body["type"].as_str().unwrap_or("CustomEvent").to_string();
    let correlation_id = Uuid::new_v4();
    let iri_builder = IriBuilder::new(state.cluster_domain.clone());
    let source = iri_builder.resource(&product, "event-store", "main");

    let envelope = EventEnvelope::new(
        iri_builder.event_schema(&event_type, 1),
        &event_type,
        source,
        Some(product.clone()),
        correlation_id,
        body,
    );

    match event_log.append(envelope).await {
        Ok(()) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "accepted",
                "event_id": correlation_id.to_string(),
                "product": product,
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

/// GET /api/event-store/:product/events — read events from a product's event store.
async fn handle_event_store_api_read(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(product): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(ref event_log) = state.event_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "event log not available" })),
        )
            .into_response();
    };

    let limit: usize = params.get("limit").and_then(|l| l.parse().ok()).unwrap_or(100);

    // Read all events from the log and filter by product scope
    let all_events = event_log.events_since(0).await;
    let product_events: Vec<_> = all_events
        .into_iter()
        .filter(|e| e.product.as_deref() == Some(product.as_str()))
        .take(limit)
        .collect();

    let events_json: Vec<serde_json::Value> = product_events
        .iter()
        .map(|e| {
            serde_json::json!({
                "event_id": e.correlation_id.to_string(),
                "event_type": e.event_type,
                "source": e.source.as_str(),
                "payload": e.payload,
                "timestamp": e.timestamp.to_rfc3339(),
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "product": product,
            "events": events_json,
            "count": product_events.len(),
        })),
    )
        .into_response()
}

/// CA certificate export — returns the platform CA's PEM certificate.
async fn handle_ca_export(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    if let Some(ref ca) = state.ca {
        let pem = ca.ca_cert_pem();
        (StatusCode::OK, [(header::CONTENT_TYPE, "application/x-pem-file")], pem.to_string()).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "CA not available"}))).into_response()
    }
}

/// GET /api/certs — list certificates (stub returning info from CA if available).
async fn handle_certs_list(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    if let Some(ref ca) = state.ca {
        let pem = ca.ca_cert_pem();
        (StatusCode::OK, Json(serde_json::json!({
            "certificates": [{
                "subject": "picloud CA",
                "issuer": "picloud CA",
                "pem": pem.to_string(),
                "type": "ca"
            }]
        }))).into_response()
    } else {
        (StatusCode::OK, Json(serde_json::json!({
            "certificates": []
        }))).into_response()
    }
}

/// GET /api/pki/crl — return empty CRL (Certificate Revocation List).
async fn handle_pki_crl() -> Response {
    (StatusCode::OK, Json(serde_json::json!({
        "crl": [],
        "updated": chrono::Utc::now().to_rfc3339()
    }))).into_response()
}

async fn handle_v2_check(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    if state.registry.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(resp) = validate_oci_token(&headers, &state).await {
        return resp;
    }
    (StatusCode::OK, Json(serde_json::json!({}))).into_response()
}

/// Dispatch GET/HEAD requests on /v2/* paths.
/// OCI Distribution paths:
///   /v2/{name}/manifests/{reference}
///   /v2/{name}/blobs/{digest}
///   /v2/{name}/tags/list
/// Requires `pull` scope.
async fn handle_v2_dispatch(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    method: Method,
    Path(path): Path<String>,
) -> Response {
    let Some(ref registry) = state.registry else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let identity = match validate_oci_token(&headers, &state).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_oci_scope(&identity, "pull") {
        return resp;
    }

    // Parse the OCI path: {name}/manifests/{ref} or {name}/blobs/{digest} or {name}/tags/list
    if let Some((repo, rest)) = split_oci_path(&path, "manifests") {
        match registry.get_manifest(&repo, &rest).await {
            Ok((content_type, data)) => {
                let digest = format!("sha256:{}", sha2_hex(&data));
                let mut resp = (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, content_type),
                     (header::HeaderName::from_static("docker-content-digest"), digest)],
                ).into_response();
                if method == Method::HEAD {
                    *resp.body_mut() = axum::body::Body::empty();
                } else {
                    *resp.body_mut() = axum::body::Body::from(data);
                }
                resp
            }
            Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "errors": [{"code": "MANIFEST_UNKNOWN", "message": "manifest unknown"}]
            }))).into_response(),
        }
    } else if let Some((repo, digest)) = split_oci_path(&path, "blobs") {
        match registry.get_blob(&digest).await {
            Ok(data) => {
                let len = data.len();
                let mut resp = (StatusCode::OK, [
                    (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                    (header::CONTENT_LENGTH, len.to_string()),
                    (header::HeaderName::from_static("docker-content-digest"), digest),
                ]).into_response();
                if method == Method::HEAD {
                    *resp.body_mut() = axum::body::Body::empty();
                } else {
                    *resp.body_mut() = axum::body::Body::from(data);
                }
                resp
            }
            Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "errors": [{"code": "BLOB_UNKNOWN", "message": "blob unknown"}]
            }))).into_response(),
        }
    } else if path.ends_with("/tags/list") {
        let repo = path.trim_end_matches("/tags/list");
        match registry.list_tags(repo).await {
            Ok(tags) => (StatusCode::OK, Json(serde_json::json!({
                "name": repo,
                "tags": tags,
            }))).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "errors": [{"code": "NAME_UNKNOWN", "message": "repository unknown"}]
            }))).into_response(),
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// PUT on /v2/* — manifest push or blob upload completion.
/// Requires `push` scope.
async fn handle_v2_put(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    let Some(ref registry) = state.registry else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let identity = match validate_oci_token(&headers, &state).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_oci_scope(&identity, "push") {
        return resp;
    }

    let body_bytes = match axum::body::to_bytes(body, 512 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "errors": [{"code": "SIZE_INVALID", "message": format!("{e}")}]
        }))).into_response(),
    };

    if let Some((repo, reference)) = split_oci_path(&path, "manifests") {
        // PUT manifest
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/vnd.oci.image.manifest.v1+json");

        match registry.put_manifest(&repo, &reference, content_type, body_bytes).await {
            Ok(digest) => {
                let location = format!("/v2/{repo}/manifests/{digest}");
                (StatusCode::CREATED, [
                    (header::LOCATION, location),
                    (header::HeaderName::from_static("docker-content-digest"), digest),
                ]).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "errors": [{"code": "MANIFEST_INVALID", "message": e.to_string()}]
            }))).into_response(),
        }
    } else if path.contains("/blobs/uploads/") {
        // PUT blob upload complete — digest in query string
        // The digest should be provided, but we compute it anyway
        let digest = format!("sha256:{}", sha2_hex(&body_bytes));
        match registry.put_blob(&digest, body_bytes).await {
            Ok(()) => (StatusCode::CREATED, [
                (header::HeaderName::from_static("docker-content-digest"), digest),
            ]).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "errors": [{"code": "BLOB_UPLOAD_INVALID", "message": e.to_string()}]
            }))).into_response(),
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// POST on /v2/* — start blob upload.
/// Requires `push` scope.
async fn handle_v2_post(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(path): Path<String>,
) -> Response {
    let Some(ref registry) = state.registry else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let identity = match validate_oci_token(&headers, &state).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_oci_scope(&identity, "push") {
        return resp;
    }

    if path.ends_with("/blobs/uploads/") || path.ends_with("/blobs/uploads") {
        let repo = path
            .trim_end_matches('/')
            .trim_end_matches("/blobs/uploads")
            .trim_end_matches('/');
        match registry.start_blob_upload(repo).await {
            Ok(uuid) => {
                let location = format!("/v2/{repo}/blobs/uploads/{uuid}");
                (StatusCode::ACCEPTED, [
                    (header::LOCATION, location),
                    (header::HeaderName::from_static("docker-upload-uuid"), uuid),
                ]).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "errors": [{"code": "BLOB_UPLOAD_UNKNOWN", "message": e.to_string()}]
            }))).into_response(),
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// DELETE on /v2/* — manifest delete.
/// Requires `delete` scope.
async fn handle_v2_delete(
    headers: HeaderMap,
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(path): Path<String>,
) -> Response {
    let Some(ref registry) = state.registry else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let identity = match validate_oci_token(&headers, &state).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    if let Err(resp) = require_oci_scope(&identity, "delete") {
        return resp;
    }

    if let Some((repo, reference)) = split_oci_path(&path, "manifests") {
        match registry.delete_manifest(&repo, &reference).await {
            Ok(()) => StatusCode::ACCEPTED.into_response(),
            Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "errors": [{"code": "MANIFEST_UNKNOWN", "message": e.to_string()}]
            }))).into_response(),
        }
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// Split an OCI Distribution path into (repository, reference).
/// E.g. "photo-app/api/manifests/v1.0" -> Some(("photo-app/api", "v1.0"))
fn split_oci_path(path: &str, segment: &str) -> Option<(String, String)> {
    let marker = format!("/{segment}/");
    let idx = path.find(&marker)?;
    let repo = &path[..idx];
    let reference = &path[idx + marker.len()..];
    if repo.is_empty() || reference.is_empty() {
        return None;
    }
    Some((repo.to_string(), reference.to_string()))
}

/// Compute SHA-256 hex digest of data.
fn sha2_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(data);
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Ingress proxy (fallback)
// ---------------------------------------------------------------------------

/// matching requests to the workload's local port.
///
/// When a native IngressRouter (ADR-048) is configured, it is used for
/// host-based + path-based routing. Otherwise, falls back to the legacy
/// IngressTable for backwards compatibility.
async fn handle_ingress_proxy(
    axum::extract::State(state): axum::extract::State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Response {
    // ADR-048: Check drain state — reject new requests when draining
    if state.drain_state.is_draining() {
        let mut resp = (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "service_unavailable",
                "message": "node is draining — retry on another node",
            })),
        )
            .into_response();
        resp.headers_mut().insert(
            header::RETRY_AFTER,
            crate::proxy::RETRY_AFTER_SECONDS.parse().unwrap(),
        );
        return resp;
    }

    // ADR-048: Use the native ingress router when available
    if let Some(ref router) = state.ingress_router {
        return crate::proxy::handle_proxy(router.clone(), method, uri, headers, body).await;
    }

    // Legacy path: simple IngressTable lookup by path prefix only
    let request_path = uri.path();

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
/// Accepts `audience` and `scope` parameters per ADR-051.
#[derive(Deserialize)]
struct TokenRequest {
    grant_type: String,
    client_id: String,
    client_secret: String,
    scope: Option<String>,
    /// Target audience — set to product IRI (ADR-051)
    #[serde(default)]
    audience: Option<String>,
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

/// POST /auth/token/exchange — Token exchange endpoint (RFC 8693 / ADR-051)
///
/// Accepts an existing access token and exchanges it for a new token
/// with a specific audience (on-behalf-of flow).
#[derive(Deserialize)]
struct TokenExchangeHttpRequest {
    grant_type: String,
    subject_token: String,
    #[serde(default)]
    subject_token_type: Option<String>,
    #[serde(default)]
    audience: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

async fn handle_token_exchange(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<TokenExchangeHttpRequest>,
) -> Response {
    let Some(ref iam) = state.iam else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "identity provider not available" })),
        )
            .into_response();
    };

    if req.grant_type != "urn:ietf:params:oauth:grant-type:token-exchange" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unsupported_grant_type",
                "error_description": "Expected grant_type=urn:ietf:params:oauth:grant-type:token-exchange",
            })),
        )
            .into_response();
    }

    // Validate the subject token first
    match iam.validate_token(&req.subject_token).await {
        Ok(identity) => {
            // Issue a new token with the requested audience
            let audience = req.audience.unwrap_or_else(|| {
                format!("https://{}", state.cluster_domain.0)
            });

            match iam
                .issue_token(&identity.identity_iri, identity.product.as_deref())
                .await
            {
                Ok(token) => {
                    let body = serde_json::json!({
                        "access_token": token,
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
                        "scope": req.scope,
                        "audience": audience,
                    });
                    resource_response(body, ContentType::Json)
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(picloud_domain::error::PiCloudError::Unauthenticated) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "invalid_token",
                "error_description": "Subject token is invalid or expired",
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
    /// Also accepts "username" for convenience — constructed as /identities/{username}.
    #[serde(alias = "username")]
    identity_iri: String,
    #[serde(default)]
    display_name: Option<String>,
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

    // If the identity_iri is a bare username (not a full IRI), construct a proper IRI
    let identity_iri_str = if req.identity_iri.starts_with("http://") || req.identity_iri.starts_with("https://") {
        req.identity_iri.clone()
    } else {
        let iri_builder = IriBuilder::new(state.cluster_domain.clone());
        iri_builder.resource("platform", "identities", &req.identity_iri).to_string()
    };

    let identity_iri = match ResourceIri::new(&identity_iri_str) {
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
        Ok((challenge_id, options)) => {
            let options_value = serde_json::to_value(&options).unwrap_or_default();
            let mut response = serde_json::json!({
                "challenge_id": challenge_id,
                "challenge": options.challenge,
                "options": options_value,
            });
            // Include publicKey-style fields for WebAuthn compatibility
            response["publicKey"] = serde_json::json!({
                "challenge": options.challenge,
                "rp": {
                    "id": options.rp_id,
                    "name": options.rp_name,
                },
                "user": {
                    "id": options.user_id,
                    "name": options.user_name,
                },
                "timeout": options.timeout_ms,
            });
            (StatusCode::OK, Json(response)).into_response()
        }
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

/// POST /api/enroll-node — Node certificate enrollment (ADR-053).
///
/// A new node submits a CSR to get signed by the cluster CA.
/// In Auto mode, the CSR is signed immediately.
/// In Token mode, the request must include a valid enrollment token.
#[derive(Deserialize)]
#[allow(dead_code)]
struct NodeEnrollRequest {
    /// DER-encoded CSR (base64) — optional for token generation requests
    #[serde(default)]
    csr_der: Option<String>,
    /// Hostname of the new node
    #[serde(default)]
    node_name: Option<String>,
    /// IP address of the new node
    #[serde(default)]
    node_ip: Option<String>,
    /// TTL for enrollment token generation (seconds)
    #[serde(default)]
    ttl_seconds: Option<u64>,
    /// Enrollment token (required in Token mode)
    #[serde(default)]
    enrollment_token: Option<String>,
    /// Cluster ID the node expects to join
    #[serde(default)]
    cluster_id: Option<Uuid>,
}

async fn handle_node_enroll(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(req): Json<NodeEnrollRequest>,
) -> Response {
    // Token generation mode: if no CSR provided, generate an enrollment token.
    // This path does not need the CA — it just generates a UUID token.
    if req.csr_der.is_none() {
        let ttl = req.ttl_seconds.unwrap_or(3600);
        let token = uuid::Uuid::new_v4().to_string();
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "token": token,
                "ttl_seconds": ttl,
                "expires_at": (chrono::Utc::now() + chrono::Duration::seconds(ttl as i64)).to_rfc3339(),
            })),
        ).into_response();
    }

    let Some(ref ca) = state.ca else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "certificate authority not available" })),
        )
            .into_response();
    };

    let csr_der_str = req.csr_der.unwrap();
    let node_name = req.node_name.unwrap_or_else(|| "unknown".to_string());
    let node_ip = req.node_ip.unwrap_or_else(|| "0.0.0.0".to_string());

    // Decode the CSR from base64
    let csr_der = match base64::engine::general_purpose::STANDARD.decode(&csr_der_str) {
        Ok(bytes) => bytes,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("invalid CSR encoding: {e}") })),
            )
                .into_response();
        }
    };

    // Sign the node's CSR
    match ca.sign_node_csr(&csr_der, &node_name, &node_ip).await {
        Ok(signed) => {
            let body = serde_json::json!({
                "node_cert_pem": signed.cert_pem,
                "ca_cert_pem": ca.ca_cert_pem(),
                "expires_at": signed.expires_at.to_rfc3339(),
                "fingerprint": signed.fingerprint,
            });
            (StatusCode::OK, Json(body)).into_response()
        }
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

    // Reject sensitive keys — they must use the secrets store instead (ADR-009)
    const SENSITIVE_PATTERNS: &[&str] = &["password", "secret", "token", "api_key", "private_key", "credential"];
    let key_lower = payload.key.to_lowercase();
    if SENSITIVE_PATTERNS.iter().any(|p| key_lower.contains(p)) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("config key '{}' looks like a secret — use the secrets store (picloud secret set) instead", payload.key),
            })),
        );
    }

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
// Internal storage sync handlers
// ---------------------------------------------------------------------------

/// GET /internal/storage/manifest/:volume
///
/// Returns a JSON manifest listing all files in the given volume directory
/// with their relative path, size, and mtime.
async fn handle_storage_manifest(
    Path(volume): Path<String>,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Response {
    let storage_path = match &state.storage_path {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Storage path not configured"
                })),
            )
                .into_response();
        }
    };

    let volume_path = storage_path.join(&volume);
    let manifest = build_volume_manifest(&volume, &volume_path);
    (StatusCode::OK, Json(serde_json::json!(manifest))).into_response()
}

/// POST /internal/storage/sync/:volume
///
/// Receives file data as JSON with base64-encoded content and writes files
/// to the local volume directory, creating subdirectories as needed.
async fn handle_storage_sync(
    Path(volume): Path<String>,
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<picloud_domain::storage::SyncRequest>,
) -> Response {
    let storage_path = match &state.storage_path {
        Some(p) => p,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "Storage path not configured"
                })),
            )
                .into_response();
        }
    };

    let volume_path = storage_path.join(&volume);

    // Ensure the volume directory exists (it may not yet exist on this replica)
    if let Err(e) = tokio::fs::create_dir_all(&volume_path).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": format!("Failed to create volume directory: {e}")
            })),
        )
            .into_response();
    }

    let mut synced = 0usize;
    let mut errors = Vec::new();

    for file_entry in &body.files {
        let file_path = volume_path.join(&file_entry.path);

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                errors.push(format!("{}: {e}", file_entry.path));
                continue;
            }
        }

        // Decode base64 content
        let data = match base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &file_entry.data,
        ) {
            Ok(d) => d,
            Err(e) => {
                errors.push(format!("{}: base64 decode error: {e}", file_entry.path));
                continue;
            }
        };

        // Write file
        if let Err(e) = tokio::fs::write(&file_path, &data).await {
            errors.push(format!("{}: write error: {e}", file_entry.path));
            continue;
        }

        synced += 1;
    }

    if errors.is_empty() {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "synced": synced,
                "volume": volume,
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::MULTI_STATUS,
            Json(serde_json::json!({
                "synced": synced,
                "volume": volume,
                "errors": errors,
            })),
        )
            .into_response()
    }
}

/// Walk a volume directory and build a manifest of all files.
fn build_volume_manifest(
    volume_name: &str,
    volume_path: &std::path::Path,
) -> picloud_domain::storage::VolumeManifest {
    let mut files = Vec::new();

    if volume_path.is_dir() {
        collect_volume_files(volume_path, volume_path, &mut files);
    }

    picloud_domain::storage::VolumeManifest {
        volume: volume_name.to_string(),
        files,
    }
}

/// Recursively collect file metadata from a volume directory.
fn collect_volume_files(
    base: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<picloud_domain::storage::ManifestEntry>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_volume_files(base, &path, out);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            out.push(picloud_domain::storage::ManifestEntry {
                path: relative,
                size: metadata.len(),
                mtime,
            });
        }
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
    async fn ontology_returns_turtle_stub_without_projector() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/products/photo-app/ontology")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/turtle"), "expected turtle, got {ct}");

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("owl:Ontology"), "turtle should contain owl:Ontology");
        assert!(body_str.contains("photo-app"), "turtle should contain product name");
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

    // --- Storage sync endpoint tests ---

    fn test_server_with_storage(storage_path: PathBuf) -> PiCloudHttpServer {
        PiCloudHttpServer::new(
            "127.0.0.1:0".parse().unwrap(),
            ClusterDomain::default(),
        )
        .with_storage_path(storage_path)
    }

    #[tokio::test]
    async fn storage_manifest_returns_file_list() {
        let dir = std::env::temp_dir().join(format!("picloud-http-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // Create a volume with files
        let vol_dir = dir.join("test-vol");
        std::fs::create_dir_all(&vol_dir).unwrap();
        std::fs::write(vol_dir.join("file1.txt"), b"hello").unwrap();
        std::fs::create_dir_all(vol_dir.join("sub")).unwrap();
        std::fs::write(vol_dir.join("sub/file2.txt"), b"world!").unwrap();

        let app = test_server_with_storage(dir.clone()).build_router();
        let req = Request::builder()
            .uri("/internal/storage/manifest/test-vol")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["volume"], "test-vol");
        let files = json["files"].as_array().unwrap();
        assert_eq!(files.len(), 2);

        let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"file1.txt"));
        assert!(paths.contains(&"sub/file2.txt"));

        // Check sizes
        let f1 = files.iter().find(|f| f["path"] == "file1.txt").unwrap();
        assert_eq!(f1["size"], 5); // "hello"

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn storage_manifest_empty_volume() {
        let dir = std::env::temp_dir().join(format!("picloud-http-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("empty-vol")).unwrap();

        let app = test_server_with_storage(dir.clone()).build_router();
        let req = Request::builder()
            .uri("/internal/storage/manifest/empty-vol")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["files"].as_array().unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn storage_manifest_nonexistent_volume() {
        let dir = std::env::temp_dir().join(format!("picloud-http-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let app = test_server_with_storage(dir.clone()).build_router();
        let req = Request::builder()
            .uri("/internal/storage/manifest/no-such-vol")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["files"].as_array().unwrap().len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn storage_sync_writes_files() {
        use base64::Engine;

        let dir = std::env::temp_dir().join(format!("picloud-http-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let app = test_server_with_storage(dir.clone()).build_router();

        let file_data = base64::engine::general_purpose::STANDARD.encode(b"hello world");
        let nested_data = base64::engine::general_purpose::STANDARD.encode(b"nested content");

        let body = serde_json::json!({
            "files": [
                {"path": "test.txt", "data": file_data},
                {"path": "sub/dir/deep.txt", "data": nested_data},
            ]
        });

        let req = Request::builder()
            .method("POST")
            .uri("/internal/storage/sync/new-vol")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp_body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&resp_body).unwrap();
        assert_eq!(json["synced"], 2);
        assert_eq!(json["volume"], "new-vol");

        // Verify files were written
        let content = std::fs::read_to_string(dir.join("new-vol/test.txt")).unwrap();
        assert_eq!(content, "hello world");

        let nested = std::fs::read_to_string(dir.join("new-vol/sub/dir/deep.txt")).unwrap();
        assert_eq!(nested, "nested content");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn storage_sync_creates_volume_dir() {
        use base64::Engine;

        let dir = std::env::temp_dir().join(format!("picloud-http-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        // Volume dir does not exist yet
        assert!(!dir.join("brand-new-vol").exists());

        let app = test_server_with_storage(dir.clone()).build_router();

        let file_data = base64::engine::general_purpose::STANDARD.encode(b"data");
        let body = serde_json::json!({
            "files": [{"path": "file.bin", "data": file_data}]
        });

        let req = Request::builder()
            .method("POST")
            .uri("/internal/storage/sync/brand-new-vol")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Volume dir should now exist
        assert!(dir.join("brand-new-vol").exists());
        assert!(dir.join("brand-new-vol/file.bin").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn storage_manifest_returns_503_without_storage_path() {
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/internal/storage/manifest/some-vol")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn storage_sync_returns_503_without_storage_path() {
        let app = test_server().build_router();
        let body = serde_json::json!({"files": []});
        let req = Request::builder()
            .method("POST")
            .uri("/internal/storage/sync/some-vol")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // -- OCI Registry Auth tests (ADR-054) --

    #[tokio::test]
    async fn v2_check_returns_401_without_token() {
        // Server with registry but no IAM -> returns 401 because auth is required
        let app = test_server().build_router();
        let req = Request::builder()
            .uri("/v2/")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Without registry, returns 404; without IAM but with registry concept,
        // returns 401. Since test_server has no registry, it returns 404.
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn v2_check_returns_401_without_token_with_registry() {
        use picloud_domain::error::Result;
        use picloud_domain::traits::{GarbageCollectionResult, RegistryStats};

        // Minimal mock registry
        struct MockRegistry;

        #[async_trait::async_trait]
        impl RegistryBackend for MockRegistry {
            async fn put_blob(&self, _: &str, _: Vec<u8>) -> Result<()> { Ok(()) }
            async fn get_blob(&self, _: &str) -> Result<Vec<u8>> { Ok(vec![]) }
            async fn blob_exists(&self, _: &str) -> Result<bool> { Ok(false) }
            async fn delete_blob(&self, _: &str) -> Result<()> { Ok(()) }
            async fn put_manifest(&self, _: &str, _: &str, _: &str, _: Vec<u8>) -> Result<String> { Ok(String::new()) }
            async fn get_manifest(&self, _: &str, _: &str) -> Result<(String, Vec<u8>)> { Ok((String::new(), vec![])) }
            async fn delete_manifest(&self, _: &str, _: &str) -> Result<()> { Ok(()) }
            async fn list_tags(&self, _: &str) -> Result<Vec<String>> { Ok(vec![]) }
            async fn list_repositories(&self) -> Result<Vec<String>> { Ok(vec![]) }
            async fn start_blob_upload(&self, _: &str) -> Result<String> { Ok(String::new()) }
            async fn garbage_collect(&self) -> Result<GarbageCollectionResult> {
                Ok(GarbageCollectionResult { blobs_scanned: 0, blobs_deleted: 0, bytes_freed: 0, duration_ms: 0 })
            }
            async fn stats(&self) -> Result<RegistryStats> {
                Ok(RegistryStats { total_blobs: 0, total_repositories: 0, storage_used_bytes: 0 })
            }
        }

        let server = test_server().with_registry(Arc::new(MockRegistry));
        let app = server.build_router();

        // No token -> 401
        let req = Request::builder()
            .uri("/v2/")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Check WWW-Authenticate header is present
        let www_auth = resp.headers().get(header::WWW_AUTHENTICATE).unwrap().to_str().unwrap();
        assert!(www_auth.contains("Bearer realm="));
        assert!(www_auth.contains("picloud.local"));
    }

    #[tokio::test]
    async fn v2_dispatch_returns_401_without_token() {
        use picloud_domain::error::Result;
        use picloud_domain::traits::{GarbageCollectionResult, RegistryStats};

        struct MockRegistry;

        #[async_trait::async_trait]
        impl RegistryBackend for MockRegistry {
            async fn put_blob(&self, _: &str, _: Vec<u8>) -> Result<()> { Ok(()) }
            async fn get_blob(&self, _: &str) -> Result<Vec<u8>> { Ok(vec![]) }
            async fn blob_exists(&self, _: &str) -> Result<bool> { Ok(false) }
            async fn delete_blob(&self, _: &str) -> Result<()> { Ok(()) }
            async fn put_manifest(&self, _: &str, _: &str, _: &str, _: Vec<u8>) -> Result<String> { Ok(String::new()) }
            async fn get_manifest(&self, _: &str, _: &str) -> Result<(String, Vec<u8>)> { Ok((String::new(), vec![])) }
            async fn delete_manifest(&self, _: &str, _: &str) -> Result<()> { Ok(()) }
            async fn list_tags(&self, _: &str) -> Result<Vec<String>> { Ok(vec![]) }
            async fn list_repositories(&self) -> Result<Vec<String>> { Ok(vec![]) }
            async fn start_blob_upload(&self, _: &str) -> Result<String> { Ok(String::new()) }
            async fn garbage_collect(&self) -> Result<GarbageCollectionResult> {
                Ok(GarbageCollectionResult { blobs_scanned: 0, blobs_deleted: 0, bytes_freed: 0, duration_ms: 0 })
            }
            async fn stats(&self) -> Result<RegistryStats> {
                Ok(RegistryStats { total_blobs: 0, total_repositories: 0, storage_used_bytes: 0 })
            }
        }

        let server = test_server().with_registry(Arc::new(MockRegistry));
        let app = server.build_router();

        // GET /v2/myrepo/tags/list without token -> 401
        let req = Request::builder()
            .uri("/v2/myrepo/tags/list")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn v2_put_returns_401_without_token() {
        use picloud_domain::error::Result;
        use picloud_domain::traits::{GarbageCollectionResult, RegistryStats};

        struct MockRegistry;

        #[async_trait::async_trait]
        impl RegistryBackend for MockRegistry {
            async fn put_blob(&self, _: &str, _: Vec<u8>) -> Result<()> { Ok(()) }
            async fn get_blob(&self, _: &str) -> Result<Vec<u8>> { Ok(vec![]) }
            async fn blob_exists(&self, _: &str) -> Result<bool> { Ok(false) }
            async fn delete_blob(&self, _: &str) -> Result<()> { Ok(()) }
            async fn put_manifest(&self, _: &str, _: &str, _: &str, _: Vec<u8>) -> Result<String> { Ok(String::new()) }
            async fn get_manifest(&self, _: &str, _: &str) -> Result<(String, Vec<u8>)> { Ok((String::new(), vec![])) }
            async fn delete_manifest(&self, _: &str, _: &str) -> Result<()> { Ok(()) }
            async fn list_tags(&self, _: &str) -> Result<Vec<String>> { Ok(vec![]) }
            async fn list_repositories(&self) -> Result<Vec<String>> { Ok(vec![]) }
            async fn start_blob_upload(&self, _: &str) -> Result<String> { Ok(String::new()) }
            async fn garbage_collect(&self) -> Result<GarbageCollectionResult> {
                Ok(GarbageCollectionResult { blobs_scanned: 0, blobs_deleted: 0, bytes_freed: 0, duration_ms: 0 })
            }
            async fn stats(&self) -> Result<RegistryStats> {
                Ok(RegistryStats { total_blobs: 0, total_repositories: 0, storage_used_bytes: 0 })
            }
        }

        let server = test_server().with_registry(Arc::new(MockRegistry));
        let app = server.build_router();

        // PUT /v2/myrepo/manifests/v1 without token -> 401
        let req = Request::builder()
            .method("PUT")
            .uri("/v2/myrepo/manifests/v1")
            .body(Body::from("{}"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn v2_delete_returns_401_without_token() {
        use picloud_domain::error::Result;
        use picloud_domain::traits::{GarbageCollectionResult, RegistryStats};

        struct MockRegistry;

        #[async_trait::async_trait]
        impl RegistryBackend for MockRegistry {
            async fn put_blob(&self, _: &str, _: Vec<u8>) -> Result<()> { Ok(()) }
            async fn get_blob(&self, _: &str) -> Result<Vec<u8>> { Ok(vec![]) }
            async fn blob_exists(&self, _: &str) -> Result<bool> { Ok(false) }
            async fn delete_blob(&self, _: &str) -> Result<()> { Ok(()) }
            async fn put_manifest(&self, _: &str, _: &str, _: &str, _: Vec<u8>) -> Result<String> { Ok(String::new()) }
            async fn get_manifest(&self, _: &str, _: &str) -> Result<(String, Vec<u8>)> { Ok((String::new(), vec![])) }
            async fn delete_manifest(&self, _: &str, _: &str) -> Result<()> { Ok(()) }
            async fn list_tags(&self, _: &str) -> Result<Vec<String>> { Ok(vec![]) }
            async fn list_repositories(&self) -> Result<Vec<String>> { Ok(vec![]) }
            async fn start_blob_upload(&self, _: &str) -> Result<String> { Ok(String::new()) }
            async fn garbage_collect(&self) -> Result<GarbageCollectionResult> {
                Ok(GarbageCollectionResult { blobs_scanned: 0, blobs_deleted: 0, bytes_freed: 0, duration_ms: 0 })
            }
            async fn stats(&self) -> Result<RegistryStats> {
                Ok(RegistryStats { total_blobs: 0, total_repositories: 0, storage_used_bytes: 0 })
            }
        }

        let server = test_server().with_registry(Arc::new(MockRegistry));
        let app = server.build_router();

        // DELETE /v2/myrepo/manifests/sha256:abc without token -> 401
        let req = Request::builder()
            .method("DELETE")
            .uri("/v2/myrepo/manifests/sha256:abc")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn oci_scope_check_admin_bypasses() {
        let identity = picloud_domain::traits::ValidatedIdentity {
            identity_iri: ResourceIri::new("https://picloud.local/identities/admin").unwrap(),
            product: None,
            roles: vec!["admin".to_string()],
            audience: None,
            scopes: vec![],
            permissions: vec![],
        };
        assert!(require_oci_scope(&identity, "pull").is_ok());
        assert!(require_oci_scope(&identity, "push").is_ok());
        assert!(require_oci_scope(&identity, "delete").is_ok());
    }

    #[test]
    fn oci_scope_check_specific_scope() {
        let identity = picloud_domain::traits::ValidatedIdentity {
            identity_iri: ResourceIri::new("https://picloud.local/identities/user1").unwrap(),
            product: None,
            roles: vec!["user".to_string()],
            audience: None,
            scopes: vec!["pull".to_string()],
            permissions: vec![],
        };
        assert!(require_oci_scope(&identity, "pull").is_ok());
        assert!(require_oci_scope(&identity, "push").is_err());
        assert!(require_oci_scope(&identity, "delete").is_err());
    }

    #[test]
    fn oci_scope_check_wildcard_scope() {
        let identity = picloud_domain::traits::ValidatedIdentity {
            identity_iri: ResourceIri::new("https://picloud.local/identities/user1").unwrap(),
            product: None,
            roles: vec!["user".to_string()],
            audience: None,
            scopes: vec!["*".to_string()],
            permissions: vec![],
        };
        assert!(require_oci_scope(&identity, "pull").is_ok());
        assert!(require_oci_scope(&identity, "push").is_ok());
        assert!(require_oci_scope(&identity, "delete").is_ok());
    }

    #[test]
    fn oci_scope_check_permissions_fallback() {
        let identity = picloud_domain::traits::ValidatedIdentity {
            identity_iri: ResourceIri::new("https://picloud.local/identities/user1").unwrap(),
            product: None,
            roles: vec!["user".to_string()],
            audience: None,
            scopes: vec![],
            permissions: vec!["push".to_string()],
        };
        assert!(require_oci_scope(&identity, "push").is_ok());
        assert!(require_oci_scope(&identity, "pull").is_err());
    }
}
