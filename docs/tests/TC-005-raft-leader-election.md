---
id: TC-005
title: raft_leader_election
type: scenario
status: unimplemented
validates:
  features: []
  adrs:
  - ADR-002
phase: 1
---

bootstrap a two-node cluster. Assert exactly one node carries `picloud:hasRole picloud:Leader` in the RDF graph within 10 seconds of init.