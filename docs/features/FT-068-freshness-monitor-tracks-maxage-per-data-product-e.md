---
id: FT-068
title: Freshness monitor — tracks maxAge per data product, emits DataProductStale when breached
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-273
- TC-330
- TC-203
- TC-207
domains: []
domains-acknowledged: {}
---

## Description

The freshness monitor tracks the time since each data product's last successful projection and compares it against the declared `maxAge` SLO (ADR-056). When staleness exceeds `maxAge`, the monitor emits `DataProductSLOBreached`. When a successful projection restores freshness, `DataProductSLORestored` is emitted.

### Monitoring behaviour

- The monitor runs on the Raft leader and checks all data products periodically (default: every 30 seconds)
- For each data product, it compares `NOW() - last_refreshed_at` against `freshness.maxAge`
- If the gap exceeds `maxAge`, `DataProductSLOBreached` is emitted
- After a subsequent successful projection, `DataProductSLORestored` is emitted

### Events

| Event | When | Payload |
|---|---|---|
| `DataProductSLOBreached` | Staleness exceeds `maxAge` | data product IRI, `maxAge`, actual staleness duration |
| `DataProductSLORestored` | Projection succeeds after breach | data product IRI, refresh duration |

### RDF projection

Freshness status is projected into the cluster graph:
```turtle
<https://picloud.local/products/photo-app/data-products/photo-locations>
    picloud:freshnessStatus "breached" ;
    picloud:lastRefreshedAt "2025-07-01T11:45:00Z"^^xsd:dateTime ;
    picloud:maxAge "PT15M"^^xsd:duration .
```

### Integration with alerts

SLO breach events can trigger alert inference rules (ADR-041). Operators can create alert rules that fire on `DataProductSLOBreached` to surface stale data products alongside hardware alerts.

### `maxAge` is an SLO, not a schedule

`maxAge` declares the maximum acceptable staleness. It does not trigger periodic refreshes. Projections rebuild only on trigger events (FT-067). If trigger events stop arriving, the data product becomes stale and the monitor detects it.
