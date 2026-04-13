---
id: TC-119
title: rule_idempotency
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
---

trigger the same inference rule 3 times with the same graph state. Assert only one set of triples is produced — no duplicates.