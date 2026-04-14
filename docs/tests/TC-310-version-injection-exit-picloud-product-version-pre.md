---
id: TC-310
title: Version injection exit — PICLOUD_PRODUCT_VERSION present in workload
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc310_version_injection_exit_picloud_product_version_present_in_workload"
validates:
  features: [FT-040]
  adrs: []
phase: 2
last-run: 2026-04-14T09:14:38.192530062+00:00
---

## Description

Exit criterion for FT-040. Validates the full version injection lifecycle: binary workloads receive `PICLOUD_PRODUCT_VERSION` as an environment variable with the correct value, container workloads carry `product_version` on their spec for runtime injection, and workloads without a version are unaffected.