---
id: TC-253
title: PICLOUD_PRODUCT_VERSION env var present in workload container
type: scenario
status: passing
runner: cargo-test
runner-args: "tc253_picloud_product_version_env_var_present_in_workload_container"
validates:
  features: [FT-040]
  adrs: []
phase: 2
last-run: 2026-04-14T09:14:38.192530062+00:00
---

## Description

Verifies that the `PICLOUD_PRODUCT_VERSION` environment variable is injected into workload containers when `product_version` is set on the workload spec. Tests both binary and container (simulated) workloads, and confirms that workloads without a product version do not receive the env var.