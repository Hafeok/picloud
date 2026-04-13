---
id: TC-163
title: compiler_roundtrip
type: scenario
status: failing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
runner: picloud-test
runner-args: "compiler-roundtrip"
---

compile a representative set of `.picloud` files covering all resource types. Assert the output is valid Turtle (parseable by an RDF library), passes SHACL validation, and contains zero blank nodes.