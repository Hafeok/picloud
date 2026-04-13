---
id: TC-040
title: internal_dns_resolution
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-014
phase: 1
---

deploy two containers in the same product. From container A, resolve `{resource-B}.{product}.picloud.internal`. Assert it resolves to container B's IP within 10 seconds of `ResourceReady`.