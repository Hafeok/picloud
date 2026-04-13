---
id: TC-142
title: flag_live_update
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
---

toggle a flag from `enabled: true` to `enabled: false` via `resource apply`. Assert `FeatureFlagChanged` event emitted and SDK reflects the new state within 5 seconds without workload restart.