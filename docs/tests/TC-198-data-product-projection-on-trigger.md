---
id: TC-198
title: data_product_projection_on_trigger
type: scenario
status: passing
validates:
  features:
  - FT-067
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_projection_on_trigger
last-run: 2026-04-17T09:30:00.592088351+00:00
last-run-duration: 0.7s
---

deploy `photo-app` with a `data-product 'photo-locations'` declaring `triggers: ['PlaceResolved']`. Emit a `PlaceResolved` event. Assert the SPARQL CONSTRUCT projection runs. Assert the data product named graph (`…/data-products/photo-locations/graph`) is populated with triples. Assert a `DataProductRefreshed` event is emitted with non-zero triple count, duration, and timestamp within `freshness.maxAge`.