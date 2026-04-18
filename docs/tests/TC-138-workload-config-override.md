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
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.8s
---

declare a product-level config entry and a workload-level override for the same key. Assert the workload's effective config (via the merged endpoint) returns the workload value, not the product value.