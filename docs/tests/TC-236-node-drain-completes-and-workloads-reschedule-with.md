---
id: TC-236
title: Node drain completes and workloads reschedule within timeout
type: scenario
status: passing
runner: cargo-test
runner-args: "tc236_node_drain_completes_and_workloads_reschedule"
validates:
  features: [FT-011]
  adrs: []
phase: 4
last-run: 2026-04-15T18:06:29.839105622+00:00
last-run-duration: 0.5s
---

## Description

Set up a 3-node cluster with workloads running on one node. Initiate a drain operation on that node. Verify:

1. The node is cordoned first (no new workloads accepted).
2. All workloads are migrated to surviving nodes.
3. The drain completes within the specified timeout (30s).
4. The node transitions to Drained state.
5. No workloads remain on the drained node.
6. The node can be uncordoned to return to Active state.
7. Drain events (NodeDrainStarted, WorkloadMigrated, NodeDrainCompleted) are emitted with correct correlation IDs.