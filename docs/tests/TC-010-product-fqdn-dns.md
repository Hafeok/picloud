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
---

after `resource apply` for a Product, assert the product FQDN resolves correctly from a client that was connected before the product was deployed (tests cache invalidation path).