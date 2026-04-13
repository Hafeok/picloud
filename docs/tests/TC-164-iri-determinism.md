---
id: TC-164
title: iri_determinism
type: scenario
status: unimplemented
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
---

compile the same `.picloud` file twice. Assert the two compiled Turtle outputs are byte-identical (deterministic IRI generation).