---
id: TC-135
title: iri_namespace_uniqueness
type: scenario
status: failing
validates:
  features:
  - FT-007
  adrs:
  - ADR-042
phase: 1
runner: picloud-test
runner-args: "iri-namespace-uniqueness"
---

assert that every resource IRI in cluster A contains `picloud.local` and no IRI contains `lab.local`, and vice versa.