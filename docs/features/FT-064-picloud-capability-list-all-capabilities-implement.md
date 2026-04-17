---
id: FT-064
title: picloud capability list — all capabilities, implementors, consumers, and fulfilment status
phase: 3
status: planned
depends-on: []
adrs:
- ADR-055
tests:
- TC-271
- TC-328
domains: []
domains-acknowledged: {}
---

## Description

The `picloud capability list` CLI command surfaces all capabilities in the cluster, their implementors, consumers, and fulfilment status.

### CLI output

```bash
$ picloud capability list
CAPABILITY        VERSION  STATUS      IMPLEMENTOR  CONSUMERS
gps-to-place      1.0.0    fulfilled   photo-app    maps-app
image-classify    1.0.0    unfulfilled -            ml-app
```

### Data source

The command queries the cluster RDF graph:
```sparql
SELECT ?capability ?version ?status ?implementor ?consumer WHERE {
  ?cap a picloud:Capability ;
       picloud:version ?version .
  OPTIONAL { ?imp picloud:implements ?cap . }
  OPTIONAL { ?con picloud:requiresCapability [ picloud:capability ?cap ] . }
}
```

### Status values

- `fulfilled` — at least one implementing Product is deployed and conformant
- `unfulfilled` — no implementing Product exists; consumers are notified via `CapabilityUnfulfilled`
- `declared` — capability exists but no consumers or implementors yet
