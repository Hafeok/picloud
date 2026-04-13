---
id: ADR-032
title: Product Event Store as First-Class Storage Primitive
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Event sourcing is the platform's internal state model. Products built on PiCloud face the same state management challenges — they need durable, replayable, auditable state. Without platform support, every Product team would implement event sourcing independently, with inconsistent quality and no integration with the platform's RDF projection layer.

**Decision:** The platform exposes its event sourcing infrastructure as a managed `event-store` resource for Products. A Product declares aggregates and their event schemas. The platform provisions a replicated event log, manages aggregate streams, serves schema IRIs, and automatically projects aggregate events into the Product's RDF store.

**Resource model:**
```bicep
event-store 'photos' = {
  product: 'photo-app'
  aggregates: [
    { type: 'Photo', schema: 'schemas/photo-events.ttl' }
    { type: 'Album', schema: 'schemas/album-events.ttl' }
  ]
}
```

**Platform provisions:**
- Replicated event log scoped to the Product (same Raft-replicated infrastructure as platform log)
- Addressable aggregate streams: `https://picloud.local/products/{name}/event-store/{store}/{Type}/{id}/events`
- Schema IRIs permanently served from the platform HTTP layer
- Automatic RDF projection of aggregate events into the Product's Oxigraph named graph
- IAM-gated HTTP API for appending and reading events

**Schema contract:**
Event schemas are declared as `.ttl` or `.shacl` files deployed with the Product and bound to its version. Schemas are immutable within a Product version. Changing a schema requires a Product version bump. All past schema IRIs are served permanently — the event log remains interpretable forever.

**Rationale:**
- Products get event sourcing + RDF projection without implementing any infrastructure
- The platform eats its own cooking — Product event stores use the same mechanisms as platform state
- Schema IRIs are consistent with ADR-031 — one versioning model for all events, platform and Product alike
- Automatic RDF projection means the Product's SPARQL endpoint reflects aggregate state immediately — no custom projectors needed for standard cases
- The HTTP API is consistent with the IRI model (ADR-029) — no custom protocol

**Consequences:**
- The platform must support multi-tenant event log partitioning — platform events and Product events coexist but are scoped separately
- Custom projectors (for non-standard projection logic) are a future concern — Phase 3 ships automatic projection only
- Product event stores add to the Raft replication load — large, high-frequency event stores may require tuning