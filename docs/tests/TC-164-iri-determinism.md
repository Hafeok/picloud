---
id: TC-164
title: iri_determinism
type: scenario
status: failing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
runner: picloud-test
runner-args: "iri-determinism"
---

compile the same `.picloud` file twice. Assert the two compiled Turtle outputs are byte-identical (deterministic IRI generation).