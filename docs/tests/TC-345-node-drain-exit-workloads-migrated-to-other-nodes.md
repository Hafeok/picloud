---
id: TC-345
title: Node drain exit — workloads migrated to other nodes
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc345_node_drain_exit_workloads_migrated_to_other_nodes"
validates:
  features: [FT-092]
  adrs: []
phase: 4
last-run: 2026-04-15T18:30:11.757622514+00:00
last-run-duration: 0.6s
---

## Description

Exit-criteria test verifying the full end-to-end drain-and-migrate workflow
with event log integration and RDF projection (FT-092).

Verifies:

1. Nodes are registered in the RDF graph via NodeJoined events.
2. A cordon → drain cycle completes with all workloads migrated.
3. WorkloadMigrated events are projected into the RDF graph.
4. SPARQL queries confirm the node's drainStatus transitions (cordoned → drained).
5. The event chain (NodeCordoned → NodeDrainStarted → WorkloadMigrated × N →
   NodeDrainCompleted) is fully correlated and projected.
6. Target nodes hold all migrated workloads; the drained node holds none.
7. The drained node can be uncordoned back to Active state.