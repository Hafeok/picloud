---
id: TC-330
title: Freshness exit — DataProductStale emitted when maxAge breached
type: exit-criteria
status: passing
validates:
  features:
  - FT-068
  adrs:
  - ADR-056
phase: 3
runner: cargo-test
runner-args: "tc330_freshness_exit_data_product_stale_emitted_when_max_age_breached"
last-run: 2026-04-17T09:45:15.220777365+00:00
last-run-duration: 1.0s
---

## Description

Full lifecycle exit-criteria test for the freshness monitor (FT-068). Declares a data product with a 2-minute maxAge SLO, then walks through the complete breach/restore cycle: (1) refresh within SLO — no actions, (2) time passes beyond maxAge — Breach emitted, (3) refresh with a fresh timestamp — Restore emitted, (4) stable state — no further actions. Validates that the monitor correctly transitions between states and only emits events on state changes.