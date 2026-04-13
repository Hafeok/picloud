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
runner: cargo-test
runner-args: "tc041_cross_product_isolation"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

assert that a container in `product-A` cannot resolve `{resource}.{product-B}.picloud.internal` (internal DNS is scoped to the product namespace).