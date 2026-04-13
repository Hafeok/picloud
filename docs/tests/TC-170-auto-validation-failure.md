---
id: TC-170
title: auto_validation_failure
type: scenario
status: failing
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
runner: picloud-test
runner-args: "auto-validation-failure"
---

generate a resource that references a non-existent volume (deliberate). Assert post-generation validation reports the cross-reference error clearly.