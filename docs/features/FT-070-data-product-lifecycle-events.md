---
id: FT-070
title: Data product lifecycle events
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-274
- TC-331
domains: []
domains-acknowledged: {}
---

## Description

Data product state changes emit lifecycle events to the platform event log (ADR-056). These events are projected into the cluster RDF graph and are subscribable.

### Events

| Event | When emitted |
|---|---|
| `DataProductDeclared` | Data product resource received and validated |
| `DataProductReady` | First successful projection completed; graph is populated |
| `DataProductRefreshed` | Projection rebuilt on trigger event; includes triple count, duration |
| `DataProductSLOBreached` | Staleness exceeds declared `maxAge` (emitted by FT-068) |
| `DataProductSLORestored` | Freshness restored after breach (emitted by FT-068) |
| `DataProductFailed` | Projection failed; reason attached |
| `DataProductDeleted` | Data product and its named graph removed |

### Event payloads

All data product lifecycle events carry:
- `data_product_iri` — the data product's canonical IRI
- `product_iri` — the owning Product's IRI
- `domain_iri` — the data domain's IRI
- `version` — the data product's version
- `timestamp` — when the event occurred
