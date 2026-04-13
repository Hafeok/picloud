---
id: TC-130
title: all_builtin_rules
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-041
phase: 1
runner: cargo-test
runner-args: "all_builtin_rules"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

for each built-in alert rule (CPU temp, memory, disk, node unreachable, workload failed), trigger the threshold condition and assert the correct `AlertFired` event type and severity.