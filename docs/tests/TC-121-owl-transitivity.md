---
id: TC-121
title: owl_transitivity
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-039
phase: 1
runner: cargo-test
runner-args: "owl_transitivity"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

declare `picloud:dependsOn rdf:type owl:TransitiveProperty`. Assert that if `A dependsOn B` and `B dependsOn C`, then `A dependsOn C` is inferred and queryable.