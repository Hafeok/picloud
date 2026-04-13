---
id: TC-142
title: flag_live_update
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: cargo-test
runner-args: "flag_live_update"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

toggle a flag from `enabled: true` to `enabled: false` via `resource apply`. Assert `FeatureFlagChanged` event emitted and SDK reflects the new state within 5 seconds without workload restart.