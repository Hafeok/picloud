---
id: TC-145
title: FeatureFlagChanged
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-044
phase: 1
runner: picloud-test
runner-args: "flag-live-update"
---