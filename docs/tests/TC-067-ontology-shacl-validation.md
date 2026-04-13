---
id: TC-067
title: ontology_shacl_validation
type: scenario
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-023
phase: 1
runner: scripts/run-tc.sh
runner-args: "ontology-shacl-validation"
last-run: 2026-04-13T21:37:33.242635225+00:00
---

add a triple that violates the product's SHACL ontology to the product graph. Assert the platform rejects the update with a SHACL validation error.