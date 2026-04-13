---
id: TC-008
title: dns_resolution
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-003
phase: 1
---

after `cluster init`, assert `picloud.local` resolves to a cluster node IP from an external client on the same broadcast domain within 2 seconds.