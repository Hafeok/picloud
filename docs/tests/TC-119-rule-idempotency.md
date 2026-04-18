---
id: TC-119
title: rule_idempotency
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "rule_idempotency"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.9s
---

trigger the same inference rule 3 times with the same graph state. Assert only one set of triples is produced — no duplicates.