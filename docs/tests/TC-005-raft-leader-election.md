---
id: TC-005
title: raft_leader_election
type: scenario
status: passing
validates:
  features:
  - FT-014
  adrs:
  - ADR-002
phase: 1
runner: scripts/run-tc.sh
runner-args: "raft-leader-election"
last-run: 2026-04-13T20:49:54.762542035+00:00
---

bootstrap a two-node cluster. Assert exactly one node carries `picloud:hasRole picloud:Leader` in the RDF graph within 10 seconds of init.