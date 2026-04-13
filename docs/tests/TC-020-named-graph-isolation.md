---
id: TC-020
title: named_graph_isolation
type: scenario
status: unimplemented
validates:
  features:
  - FT-002
  adrs:
  - ADR-006
phase: 1
---

write triples to three named graphs, assert that each named graph query returns only its own triples, and that the default graph union query returns all.