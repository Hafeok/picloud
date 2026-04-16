---
id: FT-080
title: Automatic RDF projection of Product aggregate events into Product graph
phase: 3
status: complete
depends-on: []
adrs:
- ADR-032
- ADR-005
tests:
- TC-230
domains: []
domains-acknowledged: {}
---

## Description

Product aggregate events are automatically projected into the Product's Oxigraph RDF graph by platform-managed projectors (ADR-032). When a workload appends an event to its event store, the platform runs the projection and updates the Product's named graph.

### Automatic projection

1. Workload appends a `PhotoCreated` event to the `Photo/123` aggregate stream
2. Platform acknowledges the append (event is Raft-replicated)
3. Platform projector processes the event and writes triples to the Product's named graph
4. The Product's SPARQL endpoint immediately reflects the new state

### Projection model

The platform provides automatic projection for standard event patterns — resource creation, update, deletion. Triples are derived from the event's schema IRI and payload structure. The ontology files (FT-053) define the target RDF shape.

### Current aggregate state

The projected graph maintains the current state of each aggregate:
```
GET /products/photo-app/event-store/photos/Photo/123
Accept: text/turtle
```

Returns the latest projected triples for the aggregate, not the raw events.

### Custom projectors

Phase 3 ships automatic projection only. Custom projectors (for non-standard projection logic) are a future concern. Products that need custom projection can use SPARQL CONSTRUCT inference rules (FT-057) as a workaround.
