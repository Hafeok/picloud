---
id: TC-033
title: workload_identity_injection
type: scenario
status: failing
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
runner: picloud-test
runner-args: "workload-identity-injection"
---

assert that both container and binary workloads receive the same identity injection, secret injection, and volume mount treatment. Compare environment variables between the two workload types.