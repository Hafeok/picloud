---
id: TC-192
title: capability_implementor_removed_unfulfilled
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_implementor_removed_unfulfilled"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

remove the only implementing Product. Assert `CapabilityUnfulfilled` event is emitted within 10 seconds. Assert the event is delivered to all consumer Products' event buses.