---
id: ADR-056
title: Data Products and Data Domains as First-Class Analytical Sharing Primitives
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** PiCloud Products maintain internal RDF state in per-product named graphs, queryable via IAM-gated SPARQL endpoints. The original design permitted cross-product SPARQL access as the mechanism for sharing data between Products. This decision was informed by Data Mesh thinking but did not complete the model.

The problem: cross-product access to a Product's internal graph exposes operational state — the live, mutable projection of the event log. Consumers take an implicit dependency on the producer's internal schema. When the producer refactors its graph for operational reasons, consumers break silently. There is no publication decision, no contract, no SLO, and no way to discover what data is available across the cluster.

The root issue is a missing distinction between two planes:

- **Operational graph** — a Product's internal RDF state. Private. Reflects live operational data. Schema evolves freely as the domain evolves.
- **Analytical graph** — a curated, versioned, published projection of selected domain data. Stable contract. Declared freshness SLO. Explicitly shared.

This ADR also introduces **data domains** as a governance grouping that spans multiple Products. A data domain is not a deployment unit — it is an organisational and discoverability boundary that groups related data products across the cluster.

**Decision:** Introduce two new resource types:

1. `data-domain` — a cluster-scoped governance namespace that groups data products. Has a declared steward identity, sensitivity classification, and domain-level SHACL constraints applied to all member data products at `resource apply` time.
2. `data-product` — a product-scoped resource that publishes a curated, versioned analytical projection of a subset of the Product's internal graph into a separate named graph, belonging to exactly one `data-domain`.

Cross-product SPARQL access to internal Product graphs is removed. All cross-product data sharing must go through explicitly published `data-product` resources.

**Resource definitions:**

```bicep
// Cluster-scoped
data-domain 'geospatial' = {
  description: 'All location and mapping data products across the cluster'
  steward:     'identity/alice'
  sensitivity: 'internal'
}

// Product-scoped — published by photo-app, belongs to the geospatial domain
data-product 'photo-locations' = {
  product:     'photo-app'
  domain:      'geospatial'
  version:     '1.0.0'
  description: 'Geo-tagged photo locations aggregated by resolved place'
  ontology:    './data-products/photo-locations.ttl'
  shapes:      './data-products/photo-locations.shacl'
  projection:  './data-products/photo-locations.rq'
  freshness: {
    maxAge:   '15m'
    triggers: ['PlaceResolved', 'PhotoDeleted']
  }
  access: {
    visibility: 'cluster'
    roles:      ['data-consumer']
  }
}

// Consumer depends on the data product contract, not on photo-app
product 'maps-app' = {
  version:      '1.0.0'
  dataProducts: [
    { source: 'photo-app/photo-locations', minVersion: '1.0.0' }
  ]
}
```

**Architecture: named graph separation**

```
Internal operational graph:   https://picloud.local/products/photo-app/graph
Published data product graph: https://picloud.local/products/photo-app/data-products/photo-locations/graph
```

When a trigger event arrives, the platform runs the `projection` SPARQL CONSTRUCT against the internal graph and atomically replaces the data product named graph with the result. Consumers query the published graph only.

**Freshness model — push only**

Projections rebuild exclusively on declared trigger events. No polling, no scheduled refresh, no on-query materialisation. Requiring explicit triggers forces producers to reason about which state changes make the analytical output stale — this gap surfaces at design time rather than being discovered by confused consumers in production. `freshness.maxAge` is an SLO declaration, not a scheduling mechanism. The platform monitors actual staleness and emits `DataProductSLOBreached` when exceeded.

**Enforcement rules (applied at `resource apply` time):**

1. A `data-product` must declare at least one `triggers` event.
2. A `data-product` must declare `freshness.maxAge`.
3. A `data-product` must belong to exactly one `data-domain`.
4. A `data-product` must declare `ontology` or `shapes` (or both).
5. A `data-product` with `visibility: cluster` requires the declaring Product to have at least one `data-consumer` role defined in its IAM scope.
6. A consumer Product declaring `dataProducts` dependencies fails `resource apply` if the referenced data product does not exist at the required `minVersion`.
7. A `data-domain` cannot be deleted while any `data-product` is assigned to it.
8. A `data-product` cannot be deleted while any Product declares a `dataProducts` dependency on it.
9. Cross-product SPARQL queries targeting another Product's internal named graph are rejected at the HTTP layer with `403 Forbidden`.

**Composition with capabilities (ADR-055):**

A capability's output event is a first-class trigger for a data product projection rebuild. The capability is the operational act; the data product is the analytical record of accumulated results.

```bicep
data-product 'photo-locations' = {
  freshness: {
    triggers: ['PlaceResolved']   // ADR-055 capability output drives analytical refresh
  }
}
```

**Data product lifecycle events:**

- `DataDomainDeclared`, `DataDomainDeleted`
- `DataProductDeclared`, `DataProductReady`, `DataProductRefreshed`
- `DataProductSLOBreached`, `DataProductSLORestored`
- `DataProductDeleted`

**Breaking change:** Prior cross-product SPARQL access to internal graphs is removed. Existing consumers must migrate to declared `data-product` resources.

**Rationale:**
- Hard named graph separation enforces the operational/analytical boundary in the storage layer — no convention to accidentally violate
- Push-only freshness forces producers to reason about their event model at design time
- Removing direct cross-product SPARQL access eliminates the escape hatch that would make data products optional in practice
- `data-domain` provides the discoverability surface Data Mesh's self-serve principle requires

**Rejected alternatives:**
- **Direct cross-product SPARQL access with conventions** — the status quo. Conventions are not enforced. Schema coupling grows silently.
- **Event-sourced data products (consumers replay events)** — correct for some use cases but requires every consumer to maintain their own projection infrastructure.
- **Separate analytical store (Parquet/DataFusion)** — conflicts with the RDF-native architecture. Parquet is the right primitive for telemetry (ADR-046), not domain knowledge.

**Consequences:**
- `DataProductProjector` runs SPARQL CONSTRUCT projections on trigger events and manages published named graphs
- `DataProductSLOMonitor` tracks staleness against declared `maxAge` and emits breach/restore events
- The HTTP layer enforces the cross-product SPARQL access restriction
- `picloud data-product list` and `picloud data-domain list` CLI commands