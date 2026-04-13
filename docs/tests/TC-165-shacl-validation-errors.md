---
id: TC-165
title: shacl_validation_errors
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
---

submit `.picloud` files with deliberate violations (missing required field, wrong type, invalid version expression). Assert each returns a human-readable error message matching the SHACL violation translation table.