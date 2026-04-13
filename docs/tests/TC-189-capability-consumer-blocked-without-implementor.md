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
last-run: 2026-04-13T21:47:42.689812716+00:00
---

attempt to deploy `maps-app` with a `capabilities` dependency on `gps-to-place` when no Product implements it. Assert `resource apply` fails with a `CapabilityUnfulfilled` error. Assert `maps-app` is not deployed.