---
id: TC-194
title: resource apply
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "idempotent_apply"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 1.0s
failure-message: "No matching test function found (0 tests ran)"
---