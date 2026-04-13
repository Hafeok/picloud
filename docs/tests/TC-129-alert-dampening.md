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
---

fire an alert, resolve it, re-fire within 60 seconds. Assert the second `AlertFired` is suppressed (dampening window enforced). Wait 60 seconds, re-trigger. Assert `AlertFired` now emitted.