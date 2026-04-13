---
id: TC-186
title: scenario_catalogue_sync
type: scenario
status: unimplemented
validates:
  features:
  - FT-009
  adrs:
  - ADR-054
phase: 1
---

parse the scenario catalogue in `picloud-test/scenarios/`. Assert every scenario named in an ADR `Test coverage` section has a corresponding `.rs` file in the catalogue.