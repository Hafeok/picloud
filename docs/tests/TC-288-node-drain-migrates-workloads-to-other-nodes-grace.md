---
id: TC-288
title: Node drain migrates workloads to other nodes gracefully
type: scenario
status: passing
runner: cargo-test
runner-args: "tc288_node_drain_migrates_workloads_to_other_nodes_gracefully"
validates:
  features: [FT-092]
  adrs: []
phase: 4
last-run: 2026-04-15T18:30:11.757622514+00:00
last-run-duration: 0.6s
---

## Description

Scenario test for graceful workload migration during node drain (FT-092).

Sets up a 3-node cluster with mixed workloads (containers + binaries) on one
node, drains that node, and verifies:

1. All workloads are migrated to surviving nodes via round-robin distribution.
2. Migration records track source and destination for each workload.
3. Target nodes actually receive the migrated workloads.
4. Mixed workload types (containers + binaries) are handled correctly.
5. The full event chain (NodeCordoned → NodeDrainStarted → WorkloadMigrated × N → NodeDrainCompleted) is emitted with a shared correlation ID.
6. The drained node retains zero workloads after completion.

Additional sub-scenarios cover single-target migration, migration log identity
tracking, and cascading drain (draining a node that itself received migrated
workloads).