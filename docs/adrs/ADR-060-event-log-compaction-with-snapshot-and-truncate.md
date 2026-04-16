---
id: ADR-060
title: Event Log Compaction with Snapshot-and-Truncate
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:8a92ada1647260cb5f71d295f29bef7f5743183ccbc56b8980313ba1bff13325
---

**Status:** Accepted

**Context:** The event log is append-only (ADR-004) and grows without bound. A cluster running for months accumulates millions of events. The RDF graph (the read model) is always current, so old events serve only two purposes: replay (ADR-035) and audit. For operational clusters, unbounded log growth consumes disk space and increases startup time (full replay on cold start). A compaction mechanism is needed that reduces log size while preserving correctness.

**Decision:** Compaction uses a snapshot-and-truncate model. The platform creates a binary snapshot of the current RDF graph state at a specific event log offset, then truncates the log to remove events before that offset. On restart, the platform loads the snapshot and replays only the remaining events.

**Snapshot model:**
- A snapshot is a serialized copy of the Oxigraph RDF graph state at a specific log offset
- Snapshots are stored as `.snapshot` files alongside the event log
- Only the latest snapshot is retained — creating a new snapshot deletes the previous one
- Snapshot creation is atomic — a partial snapshot is never visible

**Truncation model:**
- Events before `snapshot_offset` are removed from the on-disk log file
- The log is rewritten to contain only events from `snapshot_offset + 1` onward
- The `snapshot_offset` is recorded in a `.jsonl.meta` sidecar file

**Triggers:**
- Automatic: when the log exceeds a configurable event count threshold (default: 100,000)
- Manual: `picloud cluster compact`
- Leader-only: only the Raft leader initiates compaction to ensure consistency

**Correctness invariants:**
- `snapshot_offset + remaining_events = total_logical_event_count`
- Event IDs are never reused — logical sequence continues after compaction
- The RDF graph is identical before and after compaction
- Replay (ADR-035) works from `snapshot_offset` forward; replays requesting events before the snapshot return an error with the available range

**Rationale:**
- Snapshot-and-truncate is the standard compaction model for replicated logs (used by etcd, Raft implementations)
- Preserving only the latest snapshot keeps storage overhead minimal
- Leader-only compaction avoids coordination complexity between nodes
- The meta sidecar makes recovery deterministic — load snapshot, read offset, replay remainder

**Rejected alternatives:**
- **No compaction (append forever)** — disk exhaustion is inevitable; startup time grows linearly with cluster age
- **Event deduplication / merging** — complex to implement correctly for arbitrary event types; loses audit trail granularity
- **Periodic full graph export (no log truncation)** — reduces startup time but does not reclaim disk space

**Consequences:**
- Events before the snapshot offset are permanently lost — audit queries for very old events will not be available after compaction
- Operators who need long-term audit retention should configure offsite backup (ADR-047) before enabling automatic compaction
- Compaction is a leader-only operation — during leader election, compaction is paused
- Snapshot size is bounded by the RDF graph size, not the event log size — large graphs produce large snapshots