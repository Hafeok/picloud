---
id: FT-071
title: Data domain lifecycle events
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-275
- TC-332
domains: []
domains-acknowledged: {}
---

## Description

Data domain state changes emit lifecycle events to the platform event log (ADR-056).

### Events

| Event | When emitted |
|---|---|
| `DataDomainDeclared` | Data domain resource received and validated; steward, sensitivity, and description are set |
| `DataDomainDeleted` | Data domain removed (only when no member data products exist) |

### Event payloads

- `DataDomainDeclared` — domain IRI, steward identity IRI, sensitivity classification, description
- `DataDomainDeleted` — domain IRI, timestamp

### RDF projection

Both events are projected into the cluster graph. `DataDomainDeclared` creates the domain's triples. `DataDomainDeleted` removes them.

### Cascading effects

When a data domain is declared, all pending data products that reference it can proceed with their `resource apply` validation. Data domain lifecycle events do not directly trigger data product projections — they are governance events, not analytical triggers.
