---
id: TC-033
title: workload_identity_injection
type: scenario
status: unimplemented
validates:
  features:
  - FT-005
  adrs:
  - ADR-010
phase: 1
---

assert that both container and binary workloads receive the same identity injection, secret injection, and volume mount treatment. Compare environment variables between the two workload types.