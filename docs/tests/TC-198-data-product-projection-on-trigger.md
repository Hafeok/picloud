---
id: TC-198
title: data_product_projection_on_trigger
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_projection_on_trigger
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.7s
failure-message: "No matching test function found (0 tests ran)"
---

deploy `photo-app` with a `data-product 'photo-locations'` declaring `triggers: ['PlaceResolved']`. Emit a `PlaceResolved` event. Assert the SPARQL CONSTRUCT projection runs. Assert the data product named graph (`…/data-products/photo-locations/graph`) is populated with triples. Assert a `DataProductRefreshed` event is emitted with non-zero triple count, duration, and timestamp within `freshness.maxAge`.