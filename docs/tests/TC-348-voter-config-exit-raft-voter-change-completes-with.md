---
id: TC-348
title: Voter config exit — Raft voter change completes without downtime
type: exit-criteria
status: passing
validates:
  features: [FT-095]
  adrs: [ADR-002]
phase: 4
runner: cargo-test
runner-args: "tc348_voter_config_exit_raft_voter_change_completes_without_downtime"
last-run: 2026-04-16T07:48:49.758720469+00:00
last-run-duration: 0.5s
---

## Description

Exit criteria proving voter configuration changes complete without measurable
downtime. Submits 15 client writes across a full promote/demote cycle and
asserts all succeed within a 30-second time budget.