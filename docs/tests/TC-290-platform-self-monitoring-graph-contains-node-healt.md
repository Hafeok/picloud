---
id: TC-290
title: Platform self-monitoring graph contains node health and workload state
type: scenario
status: passing
runner: cargo-test
runner-args: "tc290_platform_self_monitoring_graph_contains_node_health_and_workload_state"
validates:
  features: [FT-094]
  adrs: []
phase: 4
last-run: 2026-04-16T07:24:03.296147050+00:00
last-run-duration: 0.7s
---

## Description

Project self-monitoring check events for multiple nodes along with workload
resource events, then verify via SPARQL that:

1. Each node has a `selfMonitoringStatus` literal (healthy/degraded/unhealthy).
2. Individual health checks are linked to nodes via `hasHealthCheck` and typed
   as `HealthCheck` with `checkName`, `checkStatus`, and `checkMessage` triples.
3. A `selfMonitoringCheckedAt` xsd:dateTime timestamp is present.
4. Workload resources carry `status` and `scheduledOn` triples, queryable
   alongside node health data.
5. Cross-cutting SPARQL queries (e.g. "all degraded nodes and their checks")
   return correct results.