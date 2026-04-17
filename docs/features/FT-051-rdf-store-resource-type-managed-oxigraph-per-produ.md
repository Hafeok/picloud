---
id: FT-051
title: rdf-store resource type — managed Oxigraph per Product
phase: 3
status: complete
depends-on: []
adrs:
- ADR-019
- ADR-006
tests:
- TC-264
- TC-321
domains: []
domains-acknowledged: {}
---

## Description

Each Product that declares an `rdf-store` resource gets a dedicated Oxigraph instance managed by the platform. The instance is backed by a platform-managed block volume with `full-replication` durability, ensuring the RDF store survives node failures.

### Resource syntax

```bicep
rdf-store 'graph' = {
  product: 'photo-app'
}
```

### What the platform provisions

- A dedicated Oxigraph instance in a separate named graph (`https://picloud.local/products/{name}/graph`)
- Block storage backing with `full-replication` durability
- Automatic lifecycle management — created on `resource apply`, destroyed on Product deletion (cascading)
- IAM-gated access — only workloads within the Product's own IAM scope and platform admins can query or update

### Internal graph privacy

The Product's operational graph is **private** (ADR-056). Cross-product SPARQL access to another Product's internal graph is rejected at the HTTP layer with `403 Forbidden`. All cross-product data sharing must go through explicitly published `data-product` resources.

### Projection integration

Product events (both platform lifecycle events and product event store events) are automatically projected into this graph by platform-managed projectors. The graph is the Product's live read model — always current, always queryable.
