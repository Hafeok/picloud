---
id: TC-024
title: symbolic_reference_resolution
type: scenario
status: failing
validates:
  features:
  - FT-001
  adrs:
  - ADR-007
phase: 1
runner: picloud-test
runner-args: "symbolic_reference_resolution"
---

declare a container that references a volume by symbolic name. Assert the compiler resolves the reference and produces correct Turtle with the volume IRI.