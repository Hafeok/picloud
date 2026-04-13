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
last-run: 2026-04-13T20:16:42.071455645+00:00
---

run `picloud resource validate` on a valid deployment with no cluster connection. Assert exit code 0 and zero errors.