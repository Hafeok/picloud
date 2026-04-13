---
id: TC-118
title: reconciliation_pass
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-038
phase: 1
runner: cargo-test
runner-args: "reconciliation_pass"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

deliberately skip the triggering event during a 10-minute window. Assert the reconciliation pass fires and the inferred triples appear within 10 minutes ± 30 seconds. Assert `ReconciliationCompleted` event in log.