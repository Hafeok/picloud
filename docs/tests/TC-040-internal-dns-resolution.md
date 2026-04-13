---
id: TC-040
title: internal_dns_resolution
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-014
phase: 1
runner: picloud-test
runner-args: "internal_dns_resolution"
---

deploy two containers in the same product. From container A, resolve `{resource-B}.{product}.picloud.internal`. Assert it resolves to container B's IP within 10 seconds of `ResourceReady`.