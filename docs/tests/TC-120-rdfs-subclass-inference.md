---
id: TC-120
title: rdfs_subclass_inference
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-039
phase: 1
---

declare `picloud:ProductionContainer rdfs:subClassOf picloud:Container` in an ontology. Query `SELECT ?x WHERE { ?x a picloud:Container }`. Assert instances of `picloud:ProductionContainer` are returned.