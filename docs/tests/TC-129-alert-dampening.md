---
id: TC-129
title: alert_dampening
type: scenario
status: passing
validates:
  features:
  - FT-009
  - FT-076
  adrs:
  - ADR-041
phase: 1
runner: cargo-test
runner-args: alert_dampening
last-run: 2026-04-17T10:11:32.419130054+00:00
last-run-duration: 2.8s
---

fire an alert, resolve it, re-fire within 60 seconds. Assert the second `AlertFired` is suppressed (dampening window enforced). Wait 60 seconds, re-trigger. Assert `AlertFired` now emitted.