---
id: TC-127
title: alert_fired
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
runner-args: alert_fired
last-run: 2026-04-17T10:11:32.419130054+00:00
last-run-duration: 0.6s
---

inject a `MetricRecorded` event with `cpu_temp_celsius: 85.0` (above the 80°C critical threshold). Assert `AlertFired` event emitted within 30 seconds. Assert an `picloud:Alert` triple present in the graph with correct `alertType`, `alertSeverity`, and `alertResource`.