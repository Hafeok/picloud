---
id: TC-299
title: Binary workload exit — raw binary scheduled, started, and monitored
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc299_binary_workload_exit"
validates:
  features: [FT-028]
  adrs: [ADR-010]
phase: 1
last-run: 2026-04-13T22:31:10.891925537+00:00
---

## Description

End-to-end lifecycle validation for raw binary workloads: schedules binaries that exit with code 0 (Stopped) and non-zero (Failed), verifies environment variable injection, confirms nonexistent binaries fail to schedule, and validates resource limits are applied without crashing the spawn.