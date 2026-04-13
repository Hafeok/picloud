---
id: TC-192
title: capability_implementor_removed_unfulfilled
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_implementor_removed_unfulfilled"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

remove the only implementing Product. Assert `CapabilityUnfulfilled` event is emitted within 10 seconds. Assert the event is delivered to all consumer Products' event buses.