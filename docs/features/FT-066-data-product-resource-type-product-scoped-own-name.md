---
id: FT-066
title: data-product resource type — product-scoped, own named graph, push-triggered SPARQL CONSTRUCT projection
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-231
- TC-197
- TC-199
- TC-204
domains: []
domains-acknowledged: {}
---

## Description

A `data-product` is a product-scoped resource that publishes a curated, versioned analytical projection into a separate named graph (ADR-056). Data products are the **only** mechanism for cross-product data sharing — direct access to another Product's internal graph is blocked.

### Resource syntax

```bicep
data-product 'photo-locations' = {
  product: 'photo-app'
  domain: 'geospatial'
  version: '1.0.0'
  description: 'Geo-tagged photo locations aggregated by resolved place'
  ontology: './data-products/photo-locations.ttl'
  shapes: './data-products/photo-locations.shacl'
  projection: './data-products/photo-locations.rq'
  freshness: {
    maxAge: '15m'
    triggers: ['PlaceResolved', 'PhotoDeleted']
  }
  access: {
    visibility: 'cluster'
    roles: ['data-consumer']
  }
}
```

### Named graph separation

- Internal graph: `https://picloud.local/products/photo-app/graph` (private)
- Data product graph: `https://picloud.local/products/photo-app/data-products/photo-locations/graph` (published)

### Validation rules (at `resource apply` time)

1. Must declare at least one `triggers` event
2. Must declare `freshness.maxAge`
3. Must belong to exactly one `data-domain`
4. Must declare `ontology` or `shapes` (or both)
5. Cannot be deleted while any Product declares a `dataProducts` dependency on it

### Projection

When a trigger event arrives, the platform runs the `projection` SPARQL CONSTRUCT against the internal graph and atomically replaces the data product named graph with the result (FT-067).
