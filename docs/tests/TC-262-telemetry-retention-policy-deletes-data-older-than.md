---
id: TC-262
title: Telemetry retention policy deletes data older than configured TTL
type: scenario
status: passing
runner: cargo-test
runner-args: "tc262_telemetry_retention_policy_deletes_data_older_than_configured_ttl"
validates:
  features: [FT-049]
  adrs: [ADR-046]
phase: 2
last-run: 2026-04-15T12:19:52.881180684+00:00
last-run-duration: 0.8s
---

## Description

Scenario test verifying that the ParquetTelemetryStore enforces per-signal
retention policies. Writes telemetry data at multiple ages (72h, 48h, 12h,
now), configures different TTLs for traces (24h) and metrics (48h), enforces
the policy, and verifies that:

1. Partition directories older than the per-signal TTL are deleted
2. Non-expired partitions are preserved with data intact
3. Enforcement results report correct partition counts per signal
4. Runtime policy updates take effect on subsequent enforcement