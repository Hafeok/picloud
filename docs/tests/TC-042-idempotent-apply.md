---
id: TC-042
title: idempotent_apply
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
runner-args: "idempotent-apply"
last-run: 2026-04-17T19:41:56.446965639+00:00
last-run-duration: 0.0s
---

apply the same resource file twice in succession. Assert the second apply produces zero new events in the event log (idempotency key deduplicated).