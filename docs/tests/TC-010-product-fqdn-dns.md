---
id: TC-010
title: product_fqdn_dns
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-003
phase: 1
runner: cargo-test
runner-args: "tc010_product_fqdn_dns"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

after `resource apply` for a Product, assert the product FQDN resolves correctly from a client that was connected before the product was deployed (tests cache invalidation path).