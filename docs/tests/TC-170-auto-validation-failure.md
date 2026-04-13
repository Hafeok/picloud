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
runner: scripts/run-tc.sh
runner-args: "auto-validation-failure"
last-run: 2026-04-13T20:16:42.071455645+00:00
---

generate a resource that references a non-existent volume (deliberate). Assert post-generation validation reports the cross-reference error clearly.