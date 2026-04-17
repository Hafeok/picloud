---
id: TC-150
title: retention_enforcement
type: scenario
status: failing
validates:
  features:
  - FT-009
  adrs:
  - ADR-046
phase: 1
runner: cargo-test
runner-args: "retention_enforcement"
last-run: 2026-04-17T15:53:31.817687922+00:00
last-run-duration: 0.8s
failure-message: "No matching test function found (0 tests ran)"
---

write Parquet partitions with timestamps older than the configured retention window. Run the hourly retention cleanup task. Assert old partition directories are deleted and newer ones remain.