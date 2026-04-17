---
id: FT-079
title: Product event schema IRIs served from platform HTTP layer
phase: 3
status: planned
depends-on: []
adrs:
- ADR-031
- ADR-032
tests:
- TC-279
- TC-336
domains: []
domains-acknowledged: {}
---

## Description

Product event schema IRIs are served by the platform HTTP layer as dereferenceable, permanent resources (ADR-031, ADR-032). Every event type in a Product's event store has a schema IRI that resolves to the schema definition.

### Schema IRI format

```
https://picloud.local/products/{product}/schemas/events/{EventType}/v{n}
```

Example:
```
https://picloud.local/products/photo-app/schemas/events/PhotoCreated/v1
https://picloud.local/products/photo-app/schemas/events/PhotoDeleted/v2
```

### HTTP serving

The platform serves schema files with content negotiation:
- `text/turtle` → Turtle RDF schema
- `application/ld+json` → JSON-LD schema
- `application/json` → JSON representation

### Permanence

All past schema versions are served permanently. When a Product bumps its version and introduces a new event schema version, the previous version remains accessible. This ensures:
- Historical events in the log remain interpretable by current projectors
- Replay (FT-081) can correctly process events from any version
- External tools and SDKs can always dereference schema IRIs

### Discovery

Schema IRIs appear in event envelopes:
```json
{
  "schema": "https://picloud.local/products/photo-app/schemas/events/PhotoCreated/v1",
  "type": "PhotoCreated",
  "payload": { ... }
}
```

The cluster graph links Products to their event schemas for SPARQL discovery.
