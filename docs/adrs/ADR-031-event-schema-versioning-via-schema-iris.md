---
id: ADR-031
title: Event Schema Versioning via Schema IRIs
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** The event log is append-only and permanent. As the platform evolves, event schemas will change — fields are added, renamed, restructured. Projectors must be able to interpret events written under any past schema. Schema evolution must be explicit, auditable, and consistent with the platform's IRI-everything model.

**Decision:** Every event carries a `schema` field containing the IRI of its schema definition. Schema IRIs are versioned and permanently dereferenceable. Projectors resolve the schema IRI to understand the event payload. Old schema IRIs are never removed — they resolve forever.

**Event envelope:**
```json
{
  "id": "uuid",
  "schema": "https://picloud.local/schemas/events/ResourceReady/v2",
  "type": "ResourceReady",
  "timestamp": "2025-01-01T00:00:00Z",
  "source": "https://picloud.local/products/photo-app",
  "payload": { ... }
}
```

**Schema resources:**
Schema definitions are served by the platform at their canonical IRI with HTTP content negotiation:
```
https://picloud.local/schemas/events/ResourceReady/v1   # original schema
https://picloud.local/schemas/events/ResourceReady/v2   # updated schema
```

Each schema IRI returns a JSON Schema or SHACL document describing the event payload structure. The platform maintains all schema versions in its RDF store — they are first-class resources, not documentation.

**Evolution rules:**
- Adding fields to a payload is always backwards-compatible — increment the minor version
- Renaming, removing, or restructuring fields requires a new major version IRI
- Projectors register handlers by schema IRI — a projector that handles `v1` and `v2` has two explicit handlers, each correct for its version
- The platform ships migration utilities for common projector patterns

**Rationale:**
- Schema IRIs are dereferenceable resources — any LLM, RDF tool, or projector can fetch the schema and understand any event without out-of-band documentation
- Consistent with the IRI-everything model (ADR-029) — schemas are part of the cluster's Linked Data surface
- Schema versioning is explicit in the event log — every event permanently records which schema it was written under
- Old schema IRIs resolve forever — the log remains fully interpretable at any point in the future without consulting external documentation
- An LLM building a new projector can fetch the schema IRI directly and generate a correct handler without needing the platform source code

**Consequences:**
- The platform must serve schema IRIs as part of its HTTP layer from Phase 1 — schema IRIs appear in the first events emitted
- Schema definitions must be written before the events that use them — schemas are deployed as part of platform releases
- Projectors accumulate handlers over time as schemas evolve — this is intentional and explicit rather than hidden