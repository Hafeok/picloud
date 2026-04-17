---
id: TC-289
title: Event log compaction reduces log size while preserving snapshots
type: scenario
status: passing
runner: cargo-test
runner-args: "tc289_event_log_compaction_reduces_log_size_while_preserving_snapshots"
validates:
  features: [FT-093]
  adrs: []
phase: 4
last-run: 2026-04-17T10:24:29.831406061+00:00
last-run-duration: 1.6s
---

## Description

Scenario test for event log compaction. Verifies that compacting a persistent
event log reduces the on-disk file size while preserving snapshot metadata
(snapshot_offset) so that logical offsets remain correct. After compaction,
events that were retained must still be readable via `events_since()` using
their original logical offsets, and the snapshot offset must survive a restart
(re-open) of the persistent log.