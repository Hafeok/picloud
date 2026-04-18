---
id: TC-007
title: raft_learner_join
type: scenario
status: passing
validates:
  features:
  - FT-014
  adrs:
  - ADR-002
phase: 1
runner: scripts/run-tc.sh
runner-args: "raft-learner-join"
last-run: 2026-04-18T18:31:11.609826359+00:00
last-run-duration: 0.0s
---

add a third node as a Raft learner. Assert it appears in the RDF graph as `picloud:Learner` before being promoted to voter.