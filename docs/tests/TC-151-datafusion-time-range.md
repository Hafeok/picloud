---
id: TC-151
title: datafusion_time_range
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: cargo-test
runner-args: "datafusion_time_range"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.9s
failure-message: "No matching test function found (0 tests ran)"
---

query traces for a known 1-hour window using `WHERE start_time BETWEEN ? AND ?`. Assert only traces within the window are returned. Measure query time on 7 days of data.