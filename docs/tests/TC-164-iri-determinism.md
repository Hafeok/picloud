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
last-run: 2026-04-17T19:41:56.446965639+00:00
last-run-duration: 0.0s
---

compile the same `.picloud` file twice. Assert the two compiled Turtle outputs are byte-identical (deterministic IRI generation).