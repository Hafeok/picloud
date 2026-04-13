---
id: TC-120
title: rdfs_subclass_inference
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-039
phase: 1
runner: cargo-test
runner-args: "rdfs_subclass_inference"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

declare `picloud:ProductionContainer rdfs:subClassOf picloud:Container` in an ontology. Query `SELECT ?x WHERE { ?x a picloud:Container }`. Assert instances of `picloud:ProductionContainer` are returned.