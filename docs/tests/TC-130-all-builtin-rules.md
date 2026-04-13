---
id: TC-130
title: all_builtin_rules
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-041
phase: 1
---

for each built-in alert rule (CPU temp, memory, disk, node unreachable, workload failed), trigger the threshold condition and assert the correct `AlertFired` event type and severity.