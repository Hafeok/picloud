---
id: TC-150
title: retention_enforcement
type: scenario
status: passing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: cargo-test
runner-args: "retention_enforcement"
last-run: 2026-04-18T13:52:32.397336516+00:00
last-run-duration: 0.7s
---

write Parquet partitions with timestamps older than the configured retention window. Run the hourly retention cleanup task. Assert old partition directories are deleted and newer ones remain.