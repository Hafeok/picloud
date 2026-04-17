---
id: TC-291
title: Multi-node Raft voter configuration change completes without downtime
type: scenario
status: passing
validates:
  features: [FT-095]
  adrs: [ADR-002]
phase: 4
runner: cargo-test
runner-args: "tc291_multi_node_raft_voter_configuration_change_completes_without_downtime"
last-run: 2026-04-17T10:25:04.349149128+00:00
last-run-duration: 2.5s
---

## Description

Validates the full voter configuration change lifecycle on a multi-node Raft
cluster: add learner, promote to voter, demote to learner, and atomic voter
set swap. Client writes are verified after every transition to prove zero
downtime throughout the reconfiguration process.