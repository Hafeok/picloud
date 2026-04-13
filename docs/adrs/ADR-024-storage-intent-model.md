---
id: ADR-024
title: Storage Intent Model
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Products need storage with different characteristics. A write-intensive database needs different storage behaviour than a media archive. Traditional approaches require operators to specify replication factors and disk types directly.

**Decision:** Products declare storage intent semantically. The platform translates intent into implementation. Intent is declared as a durability tier and a performance tier on the `volume` resource.

**MVP durability tiers:**
- `full-replication` — replicated to all available nodes. Maximum durability. Only tier available in Phase 1.

**Future durability tiers (Phase 4):**
- `quorum` — replicated to majority of nodes
- `local` — single node, no replication
- `none` — ephemeral, lost on restart

**Future performance tiers (Phase 4):**
- `fast` — low-latency random read/write
- `standard` — balanced
- `archive` — sequential write optimised

**Rationale:**
- Operators express requirements, not implementation details — consistent with the cloud abstraction model
- Platform can make better placement decisions than operators (which nodes have capacity, which nodes are healthy)
- Adding new storage tiers in Phase 4 does not require changes to Product resource files — only the platform implementation changes