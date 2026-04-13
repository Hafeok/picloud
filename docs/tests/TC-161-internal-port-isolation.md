---
id: TC-161
title: internal_port_isolation
type: scenario
status: passing
validates:
  features:
  - FT-006
  adrs:
  - ADR-048
phase: 1
runner: cargo-test
runner-args: "tc161_internal_port_isolation"
last-run: 2026-04-13T20:03:21.025167245+00:00
---

declare an ingress with `internal: true`. From an external client, attempt to connect to the ingress hostname. Assert connection refused. From a workload inside the cluster, assert connection succeeds.