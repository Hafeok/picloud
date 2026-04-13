---
id: TC-041
title: cross_product_isolation
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-014
phase: 1
runner: picloud-test
runner-args: "cross_product_isolation"
---

assert that a container in `product-A` cannot resolve `{resource}.{product-B}.picloud.internal` (internal DNS is scoped to the product namespace).