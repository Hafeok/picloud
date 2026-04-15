---
id: TC-286
title: Storage intent tier quorum replicates to majority of nodes
type: scenario
status: passing
runner: cargo-test
runner-args: "tc286_storage_intent_tier_quorum_replicates_to_majority_of_nodes"
validates:
  features: [FT-090]
  adrs: [ADR-024]
phase: 4
last-run: 2026-04-15T18:10:44.223706097+00:00
last-run-duration: 0.6s
---

## Description

Scenario test for quorum durability tier replication behaviour. Allocates volumes
with each storage tier on clusters of varying sizes (1, 3, 5, 7 nodes) and verifies:

- **Quorum** tier replicates to exactly floor(N/2)+1 nodes (a strict majority)
- **Local** tier replicates to exactly 1 node (the local node only)
- **None** tier replicates to exactly 1 node (the local node only)
- **FullReplication** tier replicates to all N nodes
- Performance tiers (Fast, Archive, Standard) do not affect the replication target count