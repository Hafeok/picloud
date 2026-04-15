---
id: TC-229
title: CPU temperature alert fires and resolves
type: exit-criteria
status: passing
validates:
  features:
  - FT-075
  - FT-076
  adrs:
  - ADR-040
  - ADR-041
phase: 3
runner: cargo-test
runner-args: tc229_cpu_temperature_alert_fires_and_resolves
last-run: 2026-04-15T16:45:13.423957151+00:00
last-run-duration: 2.6s
---

## Description

Verify that the platform metrics agent with built-in alert evaluation correctly fires
AlertFired events when CPU temperature exceeds the critical threshold (>80 C) and emits
AlertResolved events when the temperature drops back below all thresholds. Also verifies
alert dampening (no duplicate fires while the condition persists).