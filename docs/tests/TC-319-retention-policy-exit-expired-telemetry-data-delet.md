---
id: TC-319
title: Retention policy exit — expired telemetry data deleted
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc319_retention_policy_exit_expired_telemetry_data_deleted"
validates:
  features: [FT-049]
  adrs: [ADR-046]
phase: 2
last-run: 2026-04-15T12:19:52.881180684+00:00
last-run-duration: 0.8s
---

## Description

Exit-criteria test — comprehensive verification that the telemetry retention
policy correctly deletes expired data while preserving non-expired data,
using per-signal TTLs.

Validates:
1. Default retention policy matches ADR-046 (traces=7d, metrics=30d, logs=7d)
2. Per-signal retention policy is configurable via set_retention_policy
3. Expired data is deleted — partition directories are physically removed
4. Non-expired data survives enforcement with values intact
5. Different signals with different TTLs are enforced independently
6. Enforcement is idempotent — running twice produces zero additional deletes
7. Enforcement returns correct metadata (signal type, count, cutoff time)
8. Empty store handles enforcement gracefully (no errors)
9. Policy can be read back after update (round-trip verification)