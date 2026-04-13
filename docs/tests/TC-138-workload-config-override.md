---
id: TC-138
title: workload_config_override
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-043
phase: 1
runner: cargo-test
runner-args: "workload_config_override"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

declare a product-level config entry and a workload-level override for the same key. Assert the workload's effective config (via the merged endpoint) returns the workload value, not the product value.