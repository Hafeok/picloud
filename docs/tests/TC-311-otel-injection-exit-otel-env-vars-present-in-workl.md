---
id: TC-311
title: OTel injection exit — OTel env vars present in workload
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc311_otel_injection_exit_otel_env_vars_present_in_workload"
validates:
  features: [FT-041]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T10:53:27.930179986+00:00
last-run-duration: 2.4s
---

## Description

Exit criterion for FT-041. Verifies the full OTel environment variable injection
lifecycle across all workload types:

1. **Binary workload** — all three OTEL_* vars are present with correct values:
   - `OTEL_SERVICE_NAME` = workload name from IRI
   - `OTEL_EXPORTER_OTLP_ENDPOINT` = cluster OTel endpoint URL
   - `OTEL_RESOURCE_ATTRIBUTES` = product name and workload IRI
2. **Container workload** — scheduling succeeds through the OTel injection path
3. **Combined** — OTel vars, user env vars, and PICLOUD_PRODUCT_VERSION all coexist
4. **Cross-product** — different products produce correct resource attributes