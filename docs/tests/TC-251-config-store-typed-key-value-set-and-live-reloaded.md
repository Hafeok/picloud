---
id: TC-251
title: Config store typed key-value set and live-reloaded by workload
type: scenario
status: passing
validates:
  features:
  - FT-038
  adrs:
  - ADR-043
phase: 2
runner: cargo-test
runner-args: "tc251_config_store_typed_key_value_set_and_live_reloaded_by_workload"
last-run: 2026-04-14T08:46:08.258416609+00:00
---

Set typed config entries (string, int, float, bool, json) with tags via the HTTP API. Verify each is stored and retrievable with correct type. Update a value (live-reload) and confirm the workload's effective config endpoint reflects the change. Verify workload override semantics via the merged config endpoint.