---
id: TC-014
title: projection_consistency
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-004
phase: 1
runner: picloud-test
runner-args: "projection-consistency"
---

after every `resource apply`, assert that the resulting RDF state (SPARQL ASK) matches the declared resource definition within the projection latency budget.