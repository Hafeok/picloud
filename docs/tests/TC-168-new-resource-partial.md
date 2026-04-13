---
id: TC-168
title: new_resource_partial
type: scenario
status: unimplemented
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
---

run `picloud new container --product photo-app` without other required flags. Assert the CLI prompts for missing required fields only. Provide values. Assert a valid file is generated.