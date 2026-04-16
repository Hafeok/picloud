---
id: TC-347
title: Self-monitoring exit — platform graph contains health data
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc347_self_monitoring_exit_platform_graph_contains_health_data"
validates:
  features: [FT-094]
  adrs: []
phase: 4
last-run: 2026-04-16T07:24:03.296147050+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria for FT-094: project a comprehensive set of self-monitoring
events into the RDF graph and verify the platform graph contains complete
health data.

Validates the end-to-end self-monitoring flow:

1. Three nodes join and receive health monitoring data.
2. All nodes are initially healthy — verified via ASK.
3. One node transitions to unhealthy — the upsert pattern replaces old
   status and check triples without accumulating stale data.
4. `selfMonitoringCheckedAt` timestamps exist for every monitored node.
5. A dashboard-style SPARQL query combines node health status with workload
   scheduling state, returning at least one row per node.
6. An unhealthy-details query finds failing nodes and their individual
   check results in a single query.
7. Distinct health check types (raft_health, projection_lag) are
   discoverable cluster-wide.