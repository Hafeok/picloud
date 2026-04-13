---
id: TC-197
title: data_product_field_validation
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: "data_product_field_validation"
---

attempt to declare a `data-product` missing each mandatory field in turn (`triggers`, `maxAge`, `domain`, `shapes`/`ontology`). Assert each attempt is rejected at `resource apply` with a specific validation error. Assert no partial resource state is created in the cluster graph.