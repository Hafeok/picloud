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
runner: cargo-test
runner-args: "tc079_workload_cert_injection"
last-run: 2026-04-13T19:13:34.645280981+00:00
---

start a container workload. Assert the workload receives its mTLS certificate as an injected file. Assert the certificate chains to the cluster CA.