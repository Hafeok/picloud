---
id: TC-189
title: capability_consumer_blocked_without_implementor
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_consumer_blocked_without_implementor"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 1.3s
---

attempt to deploy `maps-app` with a `capabilities` dependency on `gps-to-place` when no Product implements it. Assert `resource apply` fails with a `CapabilityUnfulfilled` error. Assert `maps-app` is not deployed.