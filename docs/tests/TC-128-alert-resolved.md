---
id: TC-128
title: alert_resolved
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-041
phase: 1
---

after `AlertFired`, inject a subsequent `MetricRecorded` event with `cpu_temp_celsius: 65.0` (below threshold). Assert `AlertResolved` event emitted and `picloud:Alert` triple retracted within 30 seconds.