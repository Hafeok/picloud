---
id: TC-179
title: dns_nxdomain
type: scenario
status: failing
validates:
  features:
  - FT-006
  adrs:
  - ADR-052
phase: 1
runner: picloud-test
runner-args: "dns-nxdomain"
---

query a hostname that does not exist in the cluster. Assert NXDOMAIN response with no fallthrough.