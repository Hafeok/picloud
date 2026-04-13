---
id: FT-006
title: Networking Model
phase: 1
status: planned
depends-on:
- FT-002
adrs:
- ADR-003
- ADR-014
- ADR-028
- ADR-030
- ADR-048
- ADR-052
- ADR-053
tests:
- TC-008
- TC-009
- TC-010
- TC-011
- TC-012
- TC-040
- TC-041
- TC-081
- TC-082
- TC-083
- TC-084
- TC-085
- TC-086
- TC-159
- TC-160
- TC-161
- TC-162
- TC-175
- TC-176
- TC-177
- TC-178
- TC-179
- TC-180
- TC-181
- TC-182
- TC-183
- TC-184
- TC-222
domains:
- networking
- certificates
domains-acknowledged: {}
---

### HTTP and DNS as the RDF identity layer

RDF is an HTTP-native technology. IRIs are simultaneously the identifier and the locator for every resource. PiCloud treats this as a first-class architectural constraint — every resource in the platform has a stable, dereferenceable IRI. DNS and HTTP are not networking conveniences, they are the identity layer of the entire RDF model.

The cluster runs on a single domain: `picloud.local` (configurable). Every resource in the cluster has a canonical IRI following a path-based hierarchy:

```
https://picloud.local/                                           # cluster root
https://picloud.local/nodes/pi-node-01                          # node
https://picloud.local/products/photo-app                        # product
https://picloud.local/products/photo-app/containers/api-server  # container
https://picloud.local/products/photo-app/volumes/media-store    # volume
https://picloud.local/products/photo-app/identities/api-worker  # workload identity
https://picloud.local/products/photo-app/graph                  # SPARQL endpoint
https://picloud.local/products/photo-app/ontology               # ontology file
https://picloud.local/products/photo-app/events                 # event stream
```

Every IRI is dereferenceable. The platform serves each resource IRI with HTTP content negotiation:

```
Accept: text/turtle            → Turtle RDF representation
Accept: application/ld+json    → JSON-LD representation
Accept: application/json       → Plain JSON representation
Accept: text/html              → Human-readable view (future portal)
```

This means the cluster is a Linked Data platform by construction. Any RDF tool, SPARQL client, or LLM that can dereference an IRI can navigate the entire cluster graph by following links.

### Internal DNS

The platform runs an internal DNS resolver. Every node and product is registered at its canonical hostname derived from the IRI hierarchy:

```
picloud.local               → cluster ingress
pi-node-01.picloud.local    → direct node access
```

Products and their resources are served via path routing under the cluster domain — not subdomains. A single wildcard TLS certificate is not required. Each product gets a TLS certificate for `picloud.local/products/{name}` path space, issued by the platform's built-in CA.

Workloads address each other using their canonical IRIs. The platform's internal DNS resolves `picloud.local` to the cluster ingress, which routes by path to the correct node and product.

### Service discovery

The cluster root IRI (`https://picloud.local/`) returns a Turtle or JSON-LD document describing all Products, their IRIs, their event stream endpoints, their SPARQL endpoints, and their ontology locations. This is the semantic service registry — fully navigable by following IRI links.

### Ingress

The platform manages ingress routing for all resource IRIs automatically. No explicit ingress resource is needed for platform-managed resources. For workloads that expose custom HTTP endpoints, an ingress resource maps a path under the product's IRI space:

```bicep
ingress 'api-ingress' = {
  product: 'photo-app'
  target: 'api-server'
  port: 8080
  path: '/products/photo-app/api'
  tls: true
}
```

### Workload communication and mTLS

PiCloud enforces low coupling, high cohesion at the network layer. The event bus and the SPARQL graph are intentionally separate interfaces — events for fire-and-forget domain communication, SPARQL for read queries. This decoupling means a Product can evolve its internal state without breaking event subscribers, and event subscribers can react without needing to query.

**Workload → platform event bus:** All event publishing and subscription is routed via the platform. The platform enforces IAM on every event operation and maintains the full audit trail in the event log. Transport is mTLS — the platform issues certificates to workloads at runtime.

**Workload → product SPARQL endpoint:** SPARQL queries go directly from the querying workload to the target Product's SPARQL endpoint over mTLS. The platform issues certificates to both parties. IAM is enforced at the SPARQL endpoint, not by routing through the platform. This avoids an unnecessary hop for request-response queries.

**Node-to-node communication:** All node-to-node communication (Raft replication, storage replication, event routing) uses mTLS. Certificates are issued by the platform's built-in CA at node join time. No external PKI is required.

The separation of event bus (platform-routed) and SPARQL (direct) is a deliberate expression of the platform's coupling model: events are loosely coupled by design and benefit from platform mediation; graph queries are a known dependency between two Products and benefit from directness.

---