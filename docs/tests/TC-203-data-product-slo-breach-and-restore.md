---
id: TC-203
title: data_product_slo_breach_and_restore
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_slo_breach_and_restore
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.5s
failure-message: "No matching test function found (0 tests ran)"
---

deploy a data product with `maxAge: '2m'`. Stop emitting trigger events. Wait 2 minutes 30 seconds. Assert `DataProductSLOBreached` event emitted. Resume trigger events. Assert the next successful refresh emits `DataProductSLORestored`. Assert the SLO breach is visible in the cluster RDF graph between breach and restore events.