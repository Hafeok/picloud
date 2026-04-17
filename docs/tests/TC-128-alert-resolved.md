---
id: TC-128
title: alert_resolved
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
runner-args: alert_resolved
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
---

after `AlertFired`, inject a subsequent `MetricRecorded` event with `cpu_temp_celsius: 65.0` (below threshold). Assert `AlertResolved` event emitted and `picloud:Alert` triple retracted within 30 seconds.