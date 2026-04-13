---
id: TC-121
title: owl_transitivity
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-039
phase: 1
runner: picloud-test
runner-args: "owl-transitivity"
---

declare `picloud:dependsOn rdf:type owl:TransitiveProperty`. Assert that if `A dependsOn B` and `B dependsOn C`, then `A dependsOn C` is inferred and queryable.