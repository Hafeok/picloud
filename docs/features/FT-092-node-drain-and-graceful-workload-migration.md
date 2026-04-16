---
id: FT-092
title: Node drain and graceful workload migration
phase: 4
status: complete
depends-on: []
adrs:
- ADR-059
- ADR-010
tests:
- TC-288
- TC-345
domains: []
domains-acknowledged: {}
---

## Description

Graceful workload migration during node drain operations. When a node is
drained, all running workloads (containers and binaries) are migrated to
surviving healthy nodes via round-robin distribution. Each migration is
recorded in a traceable migration log with source node, target node, workload
IRI, and workload type. The full event chain (NodeCordoned → NodeDrainStarted →
WorkloadMigrated × N → NodeDrainCompleted) is emitted with a shared
correlation ID and projected into the RDF graph for observability.

Builds on the drain infrastructure from FT-011 (Operational Maturity) by
adding migration tracking, workload placement on target nodes, and end-to-end
verification through RDF projection.
