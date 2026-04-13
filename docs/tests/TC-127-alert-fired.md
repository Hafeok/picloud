---
id: TC-127
title: alert_fired
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-041
phase: 1
---

inject a `MetricRecorded` event with `cpu_temp_celsius: 85.0` (above the 80°C critical threshold). Assert `AlertFired` event emitted within 30 seconds. Assert an `picloud:Alert` triple present in the graph with correct `alertType`, `alertSeverity`, and `alertResource`.