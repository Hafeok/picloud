---
id: TC-242
title: Raw binary workload starts and is health-checked by platform
type: scenario
status: passing
runner: cargo-test
runner-args: "tc242_raw_binary_workload_starts_and_is_health_checked"
validates:
  features: [FT-028]
  adrs: [ADR-010]
phase: 1
last-run: 2026-04-13T22:31:10.891925537+00:00
---

## Description

Schedules a raw binary workload via the ProcessScheduler, verifies it starts with a real PID and Running status, confirms the health-check loop correctly monitors the running process, and validates that the restart policy (Always) triggers a restart when the process exits.