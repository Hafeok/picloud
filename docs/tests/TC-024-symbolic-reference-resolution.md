---
id: TC-024
title: symbolic_reference_resolution
type: scenario
status: passing
validates:
  features:
  - FT-001
  adrs:
  - ADR-007
phase: 1
runner: scripts/run-tc.sh
runner-args: "symbolic_reference_resolution"
---

declare a container that references a volume by symbolic name. Assert the compiler resolves the reference and produces correct Turtle with the volume IRI.