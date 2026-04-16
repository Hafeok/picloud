---
id: FT-094
title: Platform self-monitoring via its own RDF graph
phase: 4
status: complete
depends-on: []
adrs:
- ADR-061
- ADR-040
tests:
- TC-290
- TC-347
domains: []
domains-acknowledged: {}
---

## Description

The platform monitors its own health by projecting `SelfMonitoringCheckCompleted`
events into the RDF graph. Each event carries an overall health status for a node
plus a list of individual check results (e.g. raft_health, projection_lag,
workload_state).

The RDF projector stores:
- `picloud:selfMonitoringStatus` — the overall node health (healthy/degraded/unhealthy)
- `picloud:selfMonitoringCheckedAt` — an xsd:dateTime timestamp of the last check
- `picloud:hasHealthCheck` links to individual `picloud:HealthCheck` sub-resources,
  each carrying `checkName`, `checkStatus`, and `checkMessage` triples

Combined with existing workload projections (`picloud:status`, `picloud:scheduledOn`),
operators can query the full platform state — node health and workload scheduling —
through a single SPARQL interface.
