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
last-run: 2026-04-13T20:16:42.071455645+00:00
---

apply the same resource file twice in succession. Assert the second apply produces zero new events in the event log (idempotency key deduplicated).