---
id: TC-018
title: graph_isolation
type: scenario
status: failing
validates:
  features:
  - FT-002
  adrs:
  - ADR-005
phase: 1
runner: picloud-test
runner-args: "graph-isolation"
---

assert that a SPARQL query against the platform graph does not return triples from a product named graph, and vice versa (named graph isolation).