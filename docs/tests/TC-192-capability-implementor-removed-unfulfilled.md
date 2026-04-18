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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 3.7s
---

remove the only implementing Product. Assert `CapabilityUnfulfilled` event is emitted within 10 seconds. Assert the event is delivered to all consumer Products' event buses.