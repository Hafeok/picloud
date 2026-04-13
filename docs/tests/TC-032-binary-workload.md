---
id: TC-032
title: binary_workload
type: scenario
status: unimplemented
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
---

schedule a raw ARM64 binary. Assert it starts, receives injected `PICLOUD_WORKLOAD_IDENTITY` environment variable, and is reachable via its internal DNS name.