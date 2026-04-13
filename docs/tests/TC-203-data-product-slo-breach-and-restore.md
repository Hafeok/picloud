---
id: TC-203
title: data_product_slo_breach_and_restore
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

deploy a data product with `maxAge: '2m'`. Stop emitting trigger events. Wait 2 minutes 30 seconds. Assert `DataProductSLOBreached` event emitted. Resume trigger events. Assert the next successful refresh emits `DataProductSLORestored`. Assert the SLO breach is visible in the cluster RDF graph between breach and restore events.