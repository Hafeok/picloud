---
id: TC-346
title: Log compaction exit — log size reduced, snapshots preserved
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc346_log_compaction_exit_log_size_reduced_snapshots_preserved"
validates:
  features: [FT-093]
  adrs: []
phase: 4
last-run: 2026-04-17T10:24:29.831406061+00:00
last-run-duration: 2.0s
---

## Description

Exit-criteria test for event log compaction. Validates the final invariants
after a full compaction cycle: (1) the on-disk log file is smaller than before
compaction, (2) the number of remaining events matches the expected retention
count, (3) snapshot_offset is correctly persisted in the sidecar `.jsonl.meta`
file, (4) new events can be appended after compaction and the total logical
event count is correct, and (5) all of this state survives a restart.