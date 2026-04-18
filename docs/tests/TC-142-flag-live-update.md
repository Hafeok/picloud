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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.7s
---

toggle a flag from `enabled: true` to `enabled: false` via `resource apply`. Assert `FeatureFlagChanged` event emitted and SDK reflects the new state within 5 seconds without workload restart.