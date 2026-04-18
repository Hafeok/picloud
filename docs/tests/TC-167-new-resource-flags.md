---
id: TC-167
title: new_resource_flags
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
runner: scripts/run-tc.sh
runner-args: "new-resource-flags"
last-run: 2026-04-17T19:41:56.446965639+00:00
last-run-duration: 0.0s
---

run `picloud new container` with all required flags specified. Assert a `.picloud` file is generated, is valid (auto-validation passes), and the content matches the specified flags.