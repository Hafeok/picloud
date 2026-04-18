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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.6s
---

deliberately skip the triggering event during a 10-minute window. Assert the reconciliation pass fires and the inferred triples appear within 10 minutes ± 30 seconds. Assert `ReconciliationCompleted` event in log.