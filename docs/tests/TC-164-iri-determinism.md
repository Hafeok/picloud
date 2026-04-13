---
id: TC-164
title: iri_determinism
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
runner: scripts/run-tc.sh
runner-args: "iri-determinism"
last-run: 2026-04-13T20:16:42.071455645+00:00
---

compile the same `.picloud` file twice. Assert the two compiled Turtle outputs are byte-identical (deterministic IRI generation).