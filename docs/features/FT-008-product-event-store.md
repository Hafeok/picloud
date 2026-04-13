---
id: FT-008
title: Product Event Store
phase: 2
status: in-progress
depends-on:
- FT-005
adrs:
- ADR-018
- ADR-019
- ADR-021
- ADR-023
- ADR-032
tests:
- TC-052
- TC-053
- TC-054
- TC-055
- TC-056
- TC-057
- TC-060
- TC-061
- TC-062
- TC-066
- TC-067
- TC-093
- TC-094
- TC-095
- TC-096
- TC-211
domains:
- products
- data-model
domains-acknowledged: {}
---

The platform exposes its own event sourcing infrastructure as a first-class storage primitive for Products. A developer building a Product declares an `event-store` resource and gets a fully managed event log, aggregate streams, schema versioning, and RDF projection — without building any of it.

### Resource declaration

```bicep
event-store 'photos' = {
  product: 'photo-app'
  aggregates: [
    {
      type: 'Photo'
      schema: 'schemas/photo-events.ttl'
    }
    {
      type: 'Album'
      schema: 'schemas/album-events.ttl'
    }
  ]
}
```

The `.ttl` or `.shacl` schema files are deployed with the Product and bound to its version. Schemas only change when the Product version changes — the event log for a given version is immutable in its schema contract.

### What the platform provisions

- A replicated event log scoped to the Product, backed by the same Raft-replicated infrastructure as the platform event log
- An aggregate stream per declared type — events for `Photo/123` are addressable as a coherent stream
- Schema IRIs served at `https://picloud.local/products/{name}/schemas/events/{EventType}/v{n}` — dereferenceable, permanent
- Automatic projection of aggregate events into the Product's Oxigraph RDF store via platform-managed projectors
- The projected graph is immediately queryable via the Product's SPARQL endpoint

### HTTP API

Workloads interact with the event store via HTTP — consistent with the IRI model:

```
# Append an event to an aggregate stream
POST https://picloud.local/products/photo-app/event-store/photos/Photo/123/events
Content-Type: application/json
Authorization: Bearer {workload-token}

{
  "schema": "https://picloud.local/products/photo-app/schemas/events/PhotoCreated/v1",
  "type": "PhotoCreated",
  "payload": { ... }
}

# Read an aggregate stream
GET https://picloud.local/products/photo-app/event-store/photos/Photo/123/events

# Read current aggregate state (from RDF projection)
GET https://picloud.local/products/photo-app/event-store/photos/Photo/123
Accept: text/turtle
```

All endpoints are IAM-gated using the workload's mTLS certificate and identity token.

### Schema lifecycle

Event schemas are declared as `.ttl` or `.shacl` files and deployed as part of the Product. They are bound to the Product version — a schema cannot change without a Product version change. The platform serves all past schema versions permanently, ensuring the event log remains interpretable forever.

---