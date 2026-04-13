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
---

apply the same resource file twice in succession. Assert the second apply produces zero new events in the event log (idempotency key deduplicated).