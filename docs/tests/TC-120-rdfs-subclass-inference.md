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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.8s
---

declare `picloud:ProductionContainer rdfs:subClassOf picloud:Container` in an ontology. Query `SELECT ?x WHERE { ?x a picloud:Container }`. Assert instances of `picloud:ProductionContainer` are returned.