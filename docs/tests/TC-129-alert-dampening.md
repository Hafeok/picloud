---
id: TC-129
title: alert_dampening
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-041
phase: 1
runner: cargo-test
runner-args: "alert_dampening"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

fire an alert, resolve it, re-fire within 60 seconds. Assert the second `AlertFired` is suppressed (dampening window enforced). Wait 60 seconds, re-trigger. Assert `AlertFired` now emitted.