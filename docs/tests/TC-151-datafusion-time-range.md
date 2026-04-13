---
id: TC-151
title: datafusion_time_range
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: cargo-test
runner-args: "datafusion_time_range"
last-run: 2026-04-13T21:47:42.689812716+00:00
---

query traces for a known 1-hour window using `WHERE start_time BETWEEN ? AND ?`. Assert only traces within the window are returned. Measure query time on 7 days of data.