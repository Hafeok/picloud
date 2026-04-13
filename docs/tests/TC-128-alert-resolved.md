---
id: TC-128
title: alert_resolved
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-041
phase: 1
runner: cargo-test
runner-args: "alert_resolved"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

after `AlertFired`, inject a subsequent `MetricRecorded` event with `cpu_temp_celsius: 65.0` (below threshold). Assert `AlertResolved` event emitted and `picloud:Alert` triple retracted within 30 seconds.