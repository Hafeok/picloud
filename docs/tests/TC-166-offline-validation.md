---
id: TC-166
title: offline_validation
type: scenario
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
runner: scripts/run-tc.sh
runner-args: "offline-validation"
last-run: 2026-04-17T19:13:15.469353382+00:00
last-run-duration: 0.0s
---

run `picloud resource validate` on a valid deployment with no cluster connection. Assert exit code 0 and zero errors.