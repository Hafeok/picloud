---
id: TC-118
title: reconciliation_pass
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: picloud-test
runner-args: "reconciliation-pass"
---

deliberately skip the triggering event during a 10-minute window. Assert the reconciliation pass fires and the inferred triples appear within 10 minutes ± 30 seconds. Assert `ReconciliationCompleted` event in log.