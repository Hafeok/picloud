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
---

declare an ingress with `internal: true`. From an external client, attempt to connect to the ingress hostname. Assert connection refused. From a workload inside the cluster, assert connection succeeds.