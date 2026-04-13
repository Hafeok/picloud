---
id: TC-190
title: capability_routing
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_routing"
---

deploy `photo-app` implementing `gps-to-place`. Deploy `maps-app` consuming it. Emit a `CoordinatesReceived` event from `maps-app`. Assert a `PlaceResolved` event is routed back through `photo-app` and arrives on `maps-app`'s event bus within 2 seconds.