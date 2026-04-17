---
id: TC-206
title: capability_triggers_data_product
type: scenario
status: passing
validates:
  features:
  - FT-063
  adrs:
  - ADR-056
phase: 1
runner: cargo-test
runner-args: capability_triggers_data_product
last-run: 2026-04-17T08:52:32.819120182+00:00
last-run-duration: 0.5s
---

integration test combining ADR-055 and ADR-056. Deploy `gps-to-place` capability and `photo-locations` data product with `triggers: ['PlaceResolved']`. Emit `CoordinatesReceived` via `maps-app`. Assert the capability routes to `photo-app`, `PlaceResolved` is emitted, and the `photo-locations` data product projection is rebuilt — all within 30 seconds end-to-end.