---
id: TC-309
title: Feature flags exit — flag toggled and SDK evaluation updated
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc309_feature_flags_exit_flag_toggled_and_sdk_evaluation_updated"
validates:
  features: [FT-039]
  adrs: [ADR-044]
phase: 2
last-run: 2026-04-14T08:54:30.637108235+00:00
---

## Description

Exit criteria test for feature flags. Comprehensive verification covering the full lifecycle: create flags with all six version expression operators (=, >, >=, <, <=, range), verify evaluation against product version 3.0.0, toggle flags (disable/enable), update version expressions, delete flags, and verify invalid version expressions are rejected. Confirms FeatureFlagChanged events are emitted for all operations and SDK evaluation reflects every state change.