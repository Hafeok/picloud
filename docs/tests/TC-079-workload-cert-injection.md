---
id: TC-079
title: workload_cert_injection
type: scenario
status: passing
validates:
  features:
  - FT-003
  adrs:
  - ADR-027
phase: 1
---

start a container workload. Assert the workload receives its mTLS certificate as an injected file. Assert the certificate chains to the cluster CA.