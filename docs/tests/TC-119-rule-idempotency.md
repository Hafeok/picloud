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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

trigger the same inference rule 3 times with the same graph state. Assert only one set of triples is produced — no duplicates.