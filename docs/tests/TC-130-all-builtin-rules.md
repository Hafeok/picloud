---
id: TC-130
title: all_builtin_rules
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
runner-args: all_builtin_rules
last-run: 2026-04-15T16:41:55.847537275+00:00
last-run-duration: 0.5s
---

for each built-in alert rule (CPU temp, memory, disk, node unreachable, workload failed), trigger the threshold condition and assert the correct `AlertFired` event type and severity.