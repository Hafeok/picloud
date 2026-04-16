---
id: ADR-012
title: Mounted and Raw Block Device Support
status: accepted
features:
- FT-004
- FT-019
- FT-029
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:0c2158e4e70e5f6f61e264a7b9672901907ea34621ee8c9aaf1a42833609f994
---

**Status:** Accepted

**Context:** Different workloads have different storage access requirements. Databases typically want raw block devices to manage their own filesystems. Application containers typically want mounted filesystems.

**Decision:** PiCloud supports both mounted volumes (filesystem presented at a path) and raw block devices. Both are backed by the same distributed block storage pool.

**Rationale:**
- Mounted volumes cover the majority of use cases
- Raw block devices are required for databases (PostgreSQL, RocksDB) that manage their own storage layout
- Both types use the same allocation and replication mechanisms — no storage layer duplication

**Rejected alternatives:**
- **Mounted volumes only** — excludes databases like PostgreSQL and RocksDB that require direct block device access for performance and correctness.
- **Raw block devices only** — forces every workload to manage its own filesystem, adding unnecessary complexity for the majority of use cases.