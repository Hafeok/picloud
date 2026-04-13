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
---

add a triple that violates the product's SHACL ontology to the product graph. Assert the platform rejects the update with a SHACL validation error.