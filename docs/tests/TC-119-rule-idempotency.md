---
id: TC-119
title: rule_idempotency
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "rule_idempotency"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

trigger the same inference rule 3 times with the same graph state. Assert only one set of triples is produced — no duplicates.