---
id: TC-206
title: capability_triggers_data_product
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-056
phase: 1
---

integration test combining ADR-055 and ADR-056. Deploy `gps-to-place` capability and `photo-locations` data product with `triggers: ['PlaceResolved']`. Emit `CoordinatesReceived` via `maps-app`. Assert the capability routes to `photo-app`, `PlaceResolved` is emitted, and the `photo-locations` data product projection is rebuilt — all within 30 seconds end-to-end.