---
id: TC-186
title: scenario_catalogue_sync
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-054
phase: 1
runner: cargo-test
runner-args: "scenario_catalogue_sync"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

parse the scenario catalogue in `picloud-test/scenarios/`. Assert every scenario named in an ADR `Test coverage` section has a corresponding `.rs` file in the catalogue.