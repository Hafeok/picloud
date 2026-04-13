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
---

write Parquet partitions with timestamps older than the configured retention window. Run the hourly retention cleanup task. Assert old partition directories are deleted and newer ones remain.