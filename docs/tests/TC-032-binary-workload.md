---
id: TC-032
title: binary_workload
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
runner: picloud-test
runner-args: "binary-workload"
---

schedule a raw ARM64 binary. Assert it starts, receives injected `PICLOUD_WORKLOAD_IDENTITY` environment variable, and is reachable via its internal DNS name.