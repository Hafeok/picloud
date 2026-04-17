---
id: TC-186
title: scenario_catalogue_sync
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-054
phase: 1
runner: cargo-test
runner-args: "scenario_catalogue_sync"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

parse the scenario catalogue in `picloud-test/scenarios/`. Assert every scenario named in an ADR `Test coverage` section has a corresponding `.rs` file in the catalogue.