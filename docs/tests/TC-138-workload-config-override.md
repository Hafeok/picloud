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
---

declare a product-level config entry and a workload-level override for the same key. Assert the workload's effective config (via the merged endpoint) returns the workload value, not the product value.