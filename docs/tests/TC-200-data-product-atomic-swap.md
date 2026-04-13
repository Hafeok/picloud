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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

trigger a projection rebuild while `maps-app` is issuing SPARQL queries against the data product graph at 20 queries/second. Assert zero query errors during the swap. Assert no query returns a mix of triples from the old and new projection (partial state).