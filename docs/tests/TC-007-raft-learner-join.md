---
id: TC-007
title: raft_learner_join
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-002
phase: 1
runner: picloud-test
runner-args: "raft-learner-join"
---

add a third node as a Raft learner. Assert it appears in the RDF graph as `picloud:Learner` before being promoted to voter.