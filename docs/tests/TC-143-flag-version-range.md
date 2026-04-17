---
id: TC-143
title: flag_version_range
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: cargo-test
runner-args: "flag_version_range"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

deploy flag with `version: 2..4`. Assert active for versions 2, 3, 4 and inactive for versions 1 and 5.