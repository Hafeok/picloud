---
id: FT-078
title: event-store resource type — managed event log + aggregate streams per Product
phase: 3
status: complete
depends-on: []
adrs:
- ADR-032
- ADR-004
tests:
- TC-230
domains: []
domains-acknowledged: {}
---

## Description

The `event-store` resource type gives Products a fully managed event log with aggregate streams (ADR-032). Developers declare aggregates and their event schemas; the platform provisions storage, replication, schema serving, and RDF projection.

### Resource syntax

```bicep
event-store 'photos' = {
  product: 'photo-app'
  aggregates: [
    { type: 'Photo', schema: 'schemas/photo-events.ttl' }
    { type: 'Album', schema: 'schemas/album-events.ttl' }
  ]
}
```

### What the platform provisions

- A replicated event log scoped to the Product (same Raft infrastructure as the platform log)
- An aggregate stream per declared type — events for `Photo/123` are addressable as a coherent stream
- Schema IRIs permanently served from the platform HTTP layer (FT-079)
- Automatic RDF projection into the Product's Oxigraph graph (FT-080)
- IAM-gated HTTP API for appending and reading events

### HTTP API

```
# Append an event
POST /products/photo-app/event-store/photos/Photo/123/events

# Read aggregate stream
GET /products/photo-app/event-store/photos/Photo/123/events

# Read current aggregate state (from RDF projection)
GET /products/photo-app/event-store/photos/Photo/123
Accept: text/turtle
```

### Schema immutability

Event schemas are bound to the Product version. Changing a schema requires a Product version bump. All past schema versions are served permanently — the event log remains interpretable forever.
