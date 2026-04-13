---
id: TC-042
title: idempotent_apply
type: scenario
status: failing
validates:
  features:
  - FT-001
  - FT-007
  adrs:
  - ADR-015
phase: 1
runner: picloud-test
runner-args: "idempotent-apply"
---

apply the same resource file twice in succession. Assert the second apply produces zero new events in the event log (idempotency key deduplicated).