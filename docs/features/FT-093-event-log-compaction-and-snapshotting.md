---
id: FT-093
title: Event log compaction and snapshotting
phase: 4
status: complete
depends-on: []
adrs:
- ADR-060
- ADR-004
tests:
- TC-289
- TC-346
- TC-349
domains: []
domains-acknowledged: {}
---

## Description

The event log is append-only (ADR-004) and grows indefinitely. Compaction reduces log size by creating a point-in-time snapshot of the RDF graph state, then discarding events older than the snapshot offset. Snapshotting preserves the ability to replay events from the snapshot forward.

### Compaction model

1. **Snapshot creation** — the platform serializes the current RDF graph state (Oxigraph) into a compact binary snapshot file. The snapshot captures the full materialized graph at a specific event log offset.
2. **Log truncation** — events before the snapshot offset are removed from the on-disk log file. The log file is rewritten to contain only events from `snapshot_offset + 1` onward.
3. **Metadata persistence** — the snapshot offset is recorded in a `.jsonl.meta` sidecar file alongside the event log. On restart, the platform loads the snapshot first, then replays remaining events from the truncated log.

### Snapshot format

- Snapshots are stored as `.snapshot` files alongside the event log
- Each snapshot records the event log offset it represents
- Only the latest snapshot is retained — previous snapshots are deleted after a new one is created
- Snapshot creation is atomic — a partial snapshot is never visible to readers

### Triggers

- **Automatic** — compaction runs when the event log exceeds a configurable size threshold (default: 100,000 events)
- **Manual** — operators can trigger compaction via `picloud cluster compact`
- Compaction is leader-only — only the Raft leader initiates compaction, ensuring consistency

### Invariants

- After compaction, `snapshot_offset + remaining_events = total_logical_event_count`
- New events appended after compaction continue the logical sequence — event IDs are never reused
- Replay (FT-081) works from the snapshot offset forward — replays that require events before the snapshot offset return an error with the available range
- The RDF graph is identical before and after compaction — compaction is a storage optimization, not a state change

### Recovery

On node restart:
1. Load the latest `.snapshot` file if present
2. Read `snapshot_offset` from the `.jsonl.meta` sidecar
3. Replay events from `snapshot_offset + 1` to the end of the log
4. The RDF graph is fully reconstructed

### Events

- `CompactionStarted` — emitted when compaction begins, includes current log size
- `CompactionCompleted` — emitted on success, includes events compacted, new log size, snapshot offset
- `CompactionFailed` — emitted on failure, includes error reason; the log is unchanged
