---
id: TC-179
title: dns_nxdomain
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
---

query a hostname that does not exist in the cluster. Assert NXDOMAIN response with no fallthrough.