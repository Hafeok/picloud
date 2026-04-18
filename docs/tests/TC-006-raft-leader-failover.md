---
id: TC-006
title: raft_leader_failover
type: scenario
status: passing
validates:
  features:
  - FT-014
  adrs:
  - ADR-002
phase: 1
runner: scripts/run-tc.sh
runner-args: "raft-leader-failover"
last-run: 2026-04-18T13:18:56.552889588+00:00
last-run-duration: 0.0s
---

kill the current Raft leader process via SIGKILL. Assert a new leader is elected and the `picloud:Leader` triple updated within 5 seconds. Assert the cluster continues accepting commands.