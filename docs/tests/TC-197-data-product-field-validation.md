---
id: TC-197
title: data_product_field_validation
type: scenario
status: failing
validates:
  features:
  - FT-066
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: data_product_field_validation
last-run: 2026-04-15T14:29:59.558362753+00:00
last-run-duration: 0.7s
failure-message: "No matching test function found (0 tests ran)"
---

attempt to declare a `data-product` missing each mandatory field in turn (`triggers`, `maxAge`, `domain`, `shapes`/`ontology`). Assert each attempt is rejected at `resource apply` with a specific validation error. Assert no partial resource state is created in the cluster graph.