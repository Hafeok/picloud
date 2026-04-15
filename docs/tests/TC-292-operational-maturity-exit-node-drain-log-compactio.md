---
id: TC-292
title: Operational maturity exit — node drain, log compaction, self-monitoring pass
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc292_operational_maturity_exit"
validates:
  features: [FT-011]
  adrs: [ADR-004, ADR-005, ADR-006]
phase: 4
last-run: 2026-04-15T18:06:29.839105622+00:00
last-run-duration: 0.6s
---

## Description

Exit criteria for FT-011 Operational Maturity. Verifies all three pillars work correctly together:

### Pillar 1: Node Drain
- Cordon a node → drainStatus "cordoned" appears in RDF graph.
- Drain the node → workloads migrated, NodeDrainCompleted event emitted.
- drainStatus updates to "drained" in the RDF graph.
- Node coordinator confirms Drained state.

### Pillar 2: Log Compaction
- Event log accumulates events from drain and metric operations.
- LogCompactionCompleted event is emitted and projected without error.
- Snapshot offset is tracked correctly.

### Pillar 3: Self-Monitoring
- PlatformSelfMonitor runs 3 built-in checks (raft_health, replication_status, projection_lag).
- All checks return Healthy status.
- SelfMonitoringCheckCompleted event is emitted and projected into the RDF graph.
- selfMonitoringStatus triple appears on the node IRI.
- Monitor correctly detects degraded conditions when configured.