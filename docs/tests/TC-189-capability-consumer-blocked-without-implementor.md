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
---

attempt to deploy `maps-app` with a `capabilities` dependency on `gps-to-place` when no Product implements it. Assert `resource apply` fails with a `CapabilityUnfulfilled` error. Assert `maps-app` is not deployed.