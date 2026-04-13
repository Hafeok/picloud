---
id: TC-169
title: overwrite_protection
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-050
phase: 1
runner: scripts/run-tc.sh
runner-args: "overwrite-protection"
last-run: 2026-04-13T20:16:42.071455645+00:00
---

generate a file. Run `picloud new container` targeting the same output path without `--overwrite`. Assert the CLI refuses with a clear error and the original file is unchanged.