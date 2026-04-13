---
id: TC-166
title: offline_validation
type: scenario
status: failing
validates:
  features:
  - FT-007
  adrs:
  - ADR-049
phase: 1
runner: picloud-test
runner-args: "offline-validation"
---

run `picloud resource validate` on a valid deployment with no cluster connection. Assert exit code 0 and zero errors.