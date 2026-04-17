---
id: TC-163
title: compiler_roundtrip
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
runner: scripts/run-tc.sh
runner-args: "compiler-roundtrip"
last-run: 2026-04-17T19:13:38.300193890+00:00
last-run-duration: 0.0s
---

compile a representative set of `.picloud` files covering all resource types. Assert the output is valid Turtle (parseable by an RDF library), passes SHACL validation, and contains zero blank nodes.