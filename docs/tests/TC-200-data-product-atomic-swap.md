---
id: TC-200
title: data_product_atomic_swap
type: scenario
status: failing
validates:
  features:
  - FT-067
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_atomic_swap
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.6s
failure-message: "No matching test function found (0 tests ran)"
---

trigger a projection rebuild while `maps-app` is issuing SPARQL queries against the data product graph at 20 queries/second. Assert zero query errors during the swap. Assert no query returns a mix of triples from the old and new projection (partial state).