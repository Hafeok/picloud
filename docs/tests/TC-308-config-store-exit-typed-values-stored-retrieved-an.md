---
id: TC-308
title: Config store exit — typed values stored, retrieved, and live-reloaded
type: exit-criteria
status: passing
validates:
  features:
  - FT-038
  adrs:
  - ADR-043
phase: 2
runner: cargo-test
runner-args: "tc308_config_store_exit_typed_values_stored_retrieved_and_live_reloaded"
last-run: 2026-04-14T08:46:08.258416609+00:00
---

Comprehensive exit criteria for the config store: create typed entries (string, int, float, bool, json) with tags, retrieve each with correct type, live-reload via update, delete an entry and verify 404, verify workload effective config includes product entries, confirm event emission, and validate sensitive key rejection.