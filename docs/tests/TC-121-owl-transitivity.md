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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 2.4s
---

declare `picloud:dependsOn rdf:type owl:TransitiveProperty`. Assert that if `A dependsOn B` and `B dependsOn C`, then `A dependsOn C` is inferred and queryable.