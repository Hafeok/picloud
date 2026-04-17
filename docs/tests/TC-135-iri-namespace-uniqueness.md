---
id: TC-135
title: iri_namespace_uniqueness
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-042
phase: 1
runner: scripts/run-tc.sh
runner-args: "iri-namespace-uniqueness"
last-run: 2026-04-17T19:13:38.300193890+00:00
last-run-duration: 0.0s
---

assert that every resource IRI in cluster A contains `picloud.local` and no IRI contains `lab.local`, and vice versa.