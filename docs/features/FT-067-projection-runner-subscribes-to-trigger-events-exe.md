---
id: FT-067
title: Projection runner — subscribes to trigger events, executes CONSTRUCT, shadow-swaps data product named graph
phase: 3
status: planned
depends-on: []
adrs:
- ADR-056
tests:
- TC-231
- TC-198
- TC-200
domains: []
domains-acknowledged: {}
---

## Description

The projection runner subscribes to declared trigger events for each data product and executes the SPARQL CONSTRUCT query against the Product's internal graph (ADR-056). The result atomically replaces the data product's published named graph.

### Projection flow

1. Trigger event arrives (e.g., `PlaceResolved`)
2. Runner identifies all data products that declare this event as a trigger
3. For each data product, the runner executes its `projection` SPARQL CONSTRUCT against the Product's internal graph
4. The result triples are written to a shadow named graph
5. The shadow graph is atomically swapped with the live data product graph — consumers see no partial state
6. `DataProductRefreshed` event is emitted with triple count, duration, and timestamp

### Atomic swap

During the swap, in-flight SPARQL queries against the data product graph complete against the old graph. New queries see the new graph. No query ever sees a mix of old and new triples.

### Push-only model

Projections rebuild exclusively on declared trigger events (ADR-056). No polling, no scheduled refresh, no on-query materialization. This forces producers to reason about which state changes make the analytical output stale — gaps surface at design time, not in production.

### Error handling

If the CONSTRUCT query fails (timeout, Oxigraph error), the live graph is unchanged and a `DataProductProjectionFailed` event is emitted. The freshness monitor (FT-068) will eventually detect the staleness.
