---
id: TC-200
title: data_product_atomic_swap
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: "data_product_atomic_swap"
---

trigger a projection rebuild while `maps-app` is issuing SPARQL queries against the data product graph at 20 queries/second. Assert zero query errors during the swap. Assert no query returns a mix of triples from the old and new projection (partial state).