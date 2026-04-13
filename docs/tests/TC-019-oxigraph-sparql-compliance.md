---
id: TC-019
title: oxigraph_sparql_compliance
type: scenario
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-006
phase: 1
runner: cargo-test
runner-args: "tc019_oxigraph_sparql_compliance"
---

execute a representative set of SPARQL 1.1 queries (SELECT with FILTER, ASK, CONSTRUCT, DESCRIBE, SPARQL Update INSERT, DELETE) against the embedded Oxigraph instance. Assert correct results for each.