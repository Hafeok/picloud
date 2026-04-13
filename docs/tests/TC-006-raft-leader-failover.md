---
id: TC-006
title: raft_leader_failover
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-002
phase: 1
runner: picloud-test
runner-args: "raft-leader-failover"
---

kill the current Raft leader process via SIGKILL. Assert a new leader is elected and the `picloud:Leader` triple updated within 5 seconds. Assert the cluster continues accepting commands.