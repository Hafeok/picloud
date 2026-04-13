---
id: TC-032
title: binary_workload
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
runner: scripts/run-tc.sh
runner-args: "binary-workload"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

schedule a raw ARM64 binary. Assert it starts, receives injected `PICLOUD_WORKLOAD_IDENTITY` environment variable, and is reachable via its internal DNS name.