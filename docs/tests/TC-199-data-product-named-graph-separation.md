---
id: TC-199
title: data_product_named_graph_separation
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: "data_product_named_graph_separation"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

after a projection run, query both the internal operational graph (`…/products/photo-app/graph`) and the data product graph (`…/data-products/photo-locations/graph`). Assert they are distinct named graphs. Assert the data product graph contains only triples produced by the declared CONSTRUCT query — no triples from the internal graph appear unless the CONSTRUCT explicitly produces them.