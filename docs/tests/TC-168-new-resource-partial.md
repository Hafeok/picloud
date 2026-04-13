---
id: TC-168
title: new_resource_partial
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
runner: scripts/run-tc.sh
runner-args: "new-resource-partial"
last-run: 2026-04-13T20:16:42.071455645+00:00
---

run `picloud new container --product photo-app` without other required flags. Assert the CLI prompts for missing required fields only. Provide values. Assert a valid file is generated.