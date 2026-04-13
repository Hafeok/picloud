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
---

query traces for a known 1-hour window using `WHERE start_time BETWEEN ? AND ?`. Assert only traces within the window are returned. Measure query time on 7 days of data.