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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 3.8s
---

parse the scenario catalogue in `picloud-test/scenarios/`. Assert every scenario named in an ADR `Test coverage` section has a corresponding `.rs` file in the catalogue.