---
id: ADR-004
title: Event Sourcing as Platform State Foundation
status: accepted
features: [FT-002, FT-015, FT-035, FT-093]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Distributed systems need a consistent view of cluster state across nodes. Traditional approaches use a replicated key-value store (etcd, Consul) as a state store. PiCloud takes a different approach.

**Decision:** All platform state is derived from an append-only, Raft-replicated event log. No component writes state directly. State is always a projection of events.

**Rationale:**
- Complete audit trail of every cluster operation — no separate logging infrastructure needed
- Point-in-time state reconstruction by replaying the log to any timestamp
- Natural fit for eventually consistent operations — the CLI emits commands, subscribes to results
- Aligns with the "sensing platform" vision — every change is an observable event
- Projections (RDF graph) can be rebuilt from scratch by replaying the log, making schema migrations safe
- Event sourcing is well-understood and maps cleanly to Rust's algebraic types

**Consequences:**
- All reads go to the RDF projection, not the raw event log
- The log grows indefinitely — snapshotting and compaction must be addressed (see Open Questions)
- Eventual consistency is a first-class design constraint, not a compromise

**Rejected alternatives:**
- **etcd as state store** — external dependency, eventual consistency is hidden, no inherent audit trail
- **Embedded key-value store (sled, rocksdb)** — strong consistency possible but loses event history, audit trail, and time-travel queries