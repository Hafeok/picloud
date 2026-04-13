---
id: TC-021
title: oxigraph_persistence
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-006
phase: 1
runner: picloud-test
runner-args: "oxigraph-persistence"
---

write triples, restart the `picloud-server` process, assert triples are still present (verifies persistence across process restart).