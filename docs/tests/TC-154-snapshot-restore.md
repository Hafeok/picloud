---
id: TC-154
title: snapshot_restore
type: scenario
status: failing
validates:
  features:
  - FT-004
  adrs:
  - ADR-047
phase: 1
runner: picloud-test
runner-args: "snapshot-restore"
---

write a known sentinel file to a volume. Take a snapshot. Overwrite the sentinel. Restore from snapshot. Assert the original sentinel is present.