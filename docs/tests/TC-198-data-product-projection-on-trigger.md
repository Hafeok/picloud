---
id: TC-198
title: data_product_projection_on_trigger
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

deploy `photo-app` with a `data-product 'photo-locations'` declaring `triggers: ['PlaceResolved']`. Emit a `PlaceResolved` event. Assert the SPARQL CONSTRUCT projection runs. Assert the data product named graph (`…/data-products/photo-locations/graph`) is populated with triples. Assert a `DataProductRefreshed` event is emitted with non-zero triple count, duration, and timestamp within `freshness.maxAge`.