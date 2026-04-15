---
id: TC-343
title: Storage tiers exit — quorum tier replicates to majority
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc343_storage_tiers_exit_quorum_tier_replicates_to_majority"
validates:
  features: [FT-090]
  adrs: [ADR-024]
phase: 4
last-run: 2026-04-15T18:10:44.223706097+00:00
last-run-duration: 0.6s
---

## Description

Exit criterion for FT-090 storage tiers. Exhaustively verifies the majority
invariant for quorum replication across cluster sizes 1 through 9:

- For every cluster size N, quorum count equals floor(N/2)+1
- Quorum count is always strictly more than half the cluster (data safety guarantee)
- All quorum targets are valid cluster members (subset of member list)
- All 12 durability x performance tier combinations serialize and deserialize correctly
- Quorum replication works identically for both Mounted and RawBlock volume types