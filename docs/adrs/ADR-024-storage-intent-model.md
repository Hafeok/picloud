---
id: ADR-024
title: Storage Intent Model
status: accepted
features:
- FT-004
- FT-018
- FT-090
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:660e4ea73de91378fc1def85f7fc545288f984a60c957a6742ec3b7ec6829934
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

**Rejected alternatives:**
- **Direct replication factor specification** — exposes implementation details, creates operator error risk, and requires changes to product files when the platform's storage capabilities evolve.
- **Single storage tier (one size fits all)** — fails to distinguish between a write-intensive database and a media archive, leading to suboptimal resource allocation.