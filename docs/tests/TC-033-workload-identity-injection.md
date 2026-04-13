---
id: TC-033
title: workload_identity_injection
type: scenario
status: passing
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
runner: scripts/run-tc.sh
runner-args: "workload-identity-injection"
last-run: 2026-04-13T19:48:54.098720974+00:00
---

assert that both container and binary workloads receive the same identity injection, secret injection, and volume mount treatment. Compare environment variables between the two workload types.