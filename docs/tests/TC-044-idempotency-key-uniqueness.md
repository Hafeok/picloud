---
id: TC-044
title: idempotency_key_uniqueness
type: scenario
status: passing
validates:
  features:
  - FT-001
  - FT-007
  adrs:
  - ADR-015
phase: 1
runner: scripts/run-tc.sh
runner-args: "idempotency-key-uniqueness"
---

assert that two different apply operations (different files) produce distinct idempotency keys and are not deduplicated.