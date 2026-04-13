---
id: TC-170
title: auto_validation_failure
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
---

generate a resource that references a non-existent volume (deliberate). Assert post-generation validation reports the cross-reference error clearly.