---
id: ADR-048
title: Native Ingress Router in picloud-http
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Applications built on PiCloud need HTTP routing — TLS termination, hostname-based routing, path-based routing, and port multiplexing — without depending on nginx, traefik, or any external reverse proxy. PiCloud owns the full stack: every workload, every certificate, every IRI. This means the routing table is the RDF graph, certificates come from the platform CA, and routing updates are events — not config file reloads.

**Decision:** `picloud-http` implements a native ingress router using `hyper` (already in the stack) for proxying and `rustls` (already in the stack) for TLS termination. No external proxy dependency. The router's state is rebuilt from Oxigraph on every `IngressCreated`, `IngressUpdated`, and `IngressDeleted` event. Internal ports are routed over the cluster mTLS mesh and never exposed externally.

### Why this is simpler than nginx/traefik

nginx and traefik solve routing for arbitrary external infrastructure they do not control. PiCloud controls everything — workload addresses, certificates, and routing intent are all platform state. This eliminates the hard parts:

| nginx/traefik concern | PiCloud answer |
|---|---|
| Dynamic config reload | Events update the routing table live — no config files |
| SSL certificate management | Platform CA issues all certs (ADR-030) |
| Upstream discovery | Scheduler knows every container's node and port |
| Load balancing | One upstream per ingress — scheduler handles placement |
| Access logs / metrics | OTel handles everything (ADR-045) |
| Multiple upstreams | Not needed — containers are scheduled, not pooled |

### Router state

The router maintains an in-memory routing table rebuilt from Oxigraph on ingress resource events:

```rust
/// The complete router state — rebuilt from RDF graph on every ingress event.
/// Lookups are O(1) — HashMap keyed by (host, internal) then matched by path prefix.
pub struct IngressRouter {
    /// External routes — TLS terminated, publicly reachable
    external: HashMap<String, Vec<RouteEntry>>,   // keyed by hostname
    /// Internal routes — mTLS mesh only, not externally reachable
    internal: Vec<RouteEntry>,
    /// TLS config per hostname — SNI-based certificate selection
    tls:      Arc<rustls::ServerConfig>,
}

pub struct RouteEntry {
    pub path_prefix:  String,
    pub upstream:     Upstream,
    pub product:      String,
    pub workload_iri: ResourceIri,
}

pub struct Upstream {
    /// Internal cluster address — known from scheduler state in RDF graph
    pub address: String,
    pub port:    u16,
    /// mTLS client cert for internal upstream connections
    pub client_cert: Arc<rustls::ClientConfig>,
}
```

### Request lifecycle

```
Client → TLS handshake (SNI hostname extracted)
       → Route lookup: hostname → path prefix match → Upstream
       → Proxy request via hyper client (mTLS to upstream)
       → Stream response back to client
       → OTel span closed with status and duration
```

### Routing rules

**Host-based routing** — `photos.picloud.local` routes to the `web-frontend` container:
```bicep
ingress 'photos-web' = {
  product: 'photo-app'
  target:  'web-frontend'
  port:    3000
  host:    'photos.picloud.local'
  tls:     true
}
```

**Path-based routing** — automatic for all platform resources under `picloud.local/products/...`. No ingress resource needed.

**Internal ports** — exposed only within the cluster mTLS mesh:
```bicep
ingress 'api-metrics' = {
  product:  'photo-app'
  target:   'api-server'
  port:     9090
  internal: true           // mTLS mesh only — never externally reachable
}
```

**Multiple ingresses per container** — each port gets its own ingress resource. The platform registers each independently.

### TLS — SNI-based certificate selection

Every hostname declared in an ingress resource gets a TLS certificate issued by the platform CA. The router uses SNI to select the correct certificate per connection. Certificate issuance happens at `IngressCreated` time — the router never serves a request without a valid certificate.

```rust
// SNI resolver — selects certificate based on hostname in TLS handshake
impl rustls::server::ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let hostname = client_hello.server_name()?;
        self.certs.get(hostname).cloned()
    }
}
```

### Live routing updates — event-driven

The router subscribes to the platform event stream. On relevant events it rebuilds the affected route entries from Oxigraph:

```rust
match event.event_type.as_str() {
    "IngressCreated" | "IngressUpdated" => {
        let upstream = graph.query_upstream(&event.payload)?;
        router.upsert_route(upstream);
        tls.issue_cert_if_needed(&upstream.host);
    }
    "IngressDeleted" => {
        router.remove_route(&event.payload.ingress_iri);
    }
    "WorkloadRescheduled" => {
        // Container moved to a different node — update upstream address
        router.update_upstream_address(&event.payload);
    }
    _ => {}
}
```

No config reload, no process restart. The routing table is always consistent with the RDF graph.

### Connection draining (Phase 4)

In Phase 1, when a workload reschedules, existing connections are closed and clients retry. Phase 4 adds graceful draining:
- On `WorkloadRescheduling` event, mark upstream as draining — accept no new connections
- Allow in-flight requests up to 30 seconds to complete
- On `WorkloadRescheduled` event, update upstream address, resume routing

### Implementation size

The complete ingress router fits in approximately 500 lines across three files in `picloud-http`:

```
picloud-http/src/
├── router.rs       ~200 lines  — RouteTable, lookup, upsert, remove
├── proxy.rs        ~150 lines  — hyper reverse proxy, request forwarding
├── tls.rs          ~100 lines  — SNI resolver, cert issuance, rustls config
└── ingress.rs      ~50  lines  — event subscription, router update handler
```

**Rationale:**
- No external proxy dependency — consistent with single-binary goal (ADR-001)
- The routing table is the RDF graph — no separate config format, no drift between declared intent and runtime state
- Event-driven updates mean routing is always consistent with workload state — when a container starts, it is immediately routable
- SNI-based certificate selection handles multiple hostnames on a single port cleanly
- Internal port isolation via `internal: true` solves the metrics/health/debug port exposure problem without firewall rules
- hyper and rustls are already in the dependency stack — zero new dependencies required
- ~500 lines is a well-understood, testable surface area — not a framework, just a router

**Consequences:**
- `picloud-http` gains `router.rs`, `proxy.rs`, `tls.rs`, `ingress.rs`
- The router must handle the case where an upstream is temporarily unreachable (container restarting) — return 503 with a `Retry-After` header
- WebSocket and HTTP/2 proxying require explicit support in the hyper proxy layer — add in Phase 2
- The router runs on every node — requests are handled locally where possible, forwarded to the correct node when the target container runs elsewhere