---
id: TC-189
title: capability_consumer_blocked_without_implementor
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_consumer_blocked_without_implementor"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

attempt to deploy `maps-app` with a `capabilities` dependency on `gps-to-place` when no Product implements it. Assert `resource apply` fails with a `CapabilityUnfulfilled` error. Assert `maps-app` is not deployed.