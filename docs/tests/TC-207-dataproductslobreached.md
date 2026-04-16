---
id: TC-207
title: DataProductSLOBreached
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
failure-message: No matching test function found (0 tests ran)
last-run-duration: 0.5s
---

Deploy a data product with `maxAge: '2m'`. Stop emitting trigger events. Wait for staleness to exceed `maxAge`. Assert `DataProductSLOBreached` event is emitted with the correct data product IRI, the declared `maxAge`, and the actual staleness duration. Assert the breach is reflected in the cluster RDF graph with `picloud:freshnessStatus "breached"`.

This test validates the SLO breach detection side of the freshness monitor (FT-068). It complements TC-203 which also tests the restore cycle.