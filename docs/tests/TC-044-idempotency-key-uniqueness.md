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
last-run: 2026-04-17T15:53:13.142368276+00:00
last-run-duration: 0.0s
---

assert that two different apply operations (different files) produce distinct idempotency keys and are not deduplicated.