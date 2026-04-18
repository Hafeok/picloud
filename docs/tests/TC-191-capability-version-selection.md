---
id: TC-191
title: capability_version_selection
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-055
phase: 1
runner: cargo-test
runner-args: "capability_version_selection"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 3.2s
---

deploy two Products implementing `gps-to-place` at `v1.0.0` and `v1.1.0`. Deploy a consumer requiring `minVersion: '1.0.0'`. Emit `CoordinatesReceived`. Assert the `v1.1.0` implementor handles the event (highest satisfying version wins).