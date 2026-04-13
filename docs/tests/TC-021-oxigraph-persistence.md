---
id: TC-021
title: oxigraph_persistence
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-006
phase: 1
runner: cargo-test
runner-args: "tc021_oxigraph_persistence"
---

write triples, restart the `picloud-server` process, assert triples are still present (verifies persistence across process restart).