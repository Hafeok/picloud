---
id: TC-252
title: Feature flag toggled and SDK evaluation reflects new state
type: scenario
status: passing
runner: cargo-test
runner-args: "tc252_feature_flag_toggled_and_sdk_evaluation_reflects_new_state"
validates:
  features: [FT-039]
  adrs: [ADR-044]
phase: 2
last-run: 2026-04-14T08:54:30.637108235+00:00
---

## Description

Scenario test for feature flag version-bound evaluation and live toggling. Creates a product at version 2.1.0 with four feature flags using different version expression operators (=, >=, <, range). Verifies that SDK evaluation (GET /products/:name/flags/:flag) returns correct active/inactive state based on version matching. Then toggles a flag (disables and re-enables) and confirms the evaluation endpoint reflects the updated state without restart, proving event invalidation works.