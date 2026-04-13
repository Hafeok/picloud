---
id: TC-044
title: idempotency_key_uniqueness
type: scenario
status: unimplemented
validates:
  features:
  - FT-001
  - FT-007
  adrs:
  - ADR-015
phase: 1
---

assert that two different apply operations (different files) produce distinct idempotency keys and are not deduplicated.