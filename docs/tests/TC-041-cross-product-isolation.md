---
id: TC-041
title: cross_product_isolation
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-014
phase: 1
---

assert that a container in `product-A` cannot resolve `{resource}.{product-B}.picloud.internal` (internal DNS is scoped to the product namespace).