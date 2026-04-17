---
id: FT-073
title: picloud data-product list and picloud data-domain list
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-277
- TC-334
domains: []
domains-acknowledged: {}
---

## Description

CLI commands for listing data products and data domains, showing mesh topology and freshness status.

### `picloud data-product list`

```bash
$ picloud data-product list
DATA PRODUCT              DOMAIN       PRODUCER    STATUS    FRESHNESS   CONSUMERS
photo-app/photo-locations geospatial   photo-app   healthy   3m ago      maps-app
user-service/user-stats   analytics    user-svc    breached  45m ago     dashboard
```

### `picloud data-domain list`

```bash
$ picloud data-domain list
DOMAIN       STEWARD   SENSITIVITY  DATA PRODUCTS
geospatial   alice     internal     1
analytics    bob       internal     1
```

### Data source

Both commands query the cluster RDF graph — the same SPARQL queries that any workload could run. The CLI formats the results as tables.

### Freshness display

- `healthy` — within `maxAge` SLO
- `breached` — staleness exceeds `maxAge`; shows actual staleness duration
- `unknown` — no projection has completed yet
