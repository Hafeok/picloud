---
id: ADR-012
title: Mounted and Raw Block Device Support
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Different workloads have different storage access requirements. Databases typically want raw block devices to manage their own filesystems. Application containers typically want mounted filesystems.

**Decision:** PiCloud supports both mounted volumes (filesystem presented at a path) and raw block devices. Both are backed by the same distributed block storage pool.

**Rationale:**
- Mounted volumes cover the majority of use cases
- Raw block devices are required for databases (PostgreSQL, RocksDB) that manage their own storage layout
- Both types use the same allocation and replication mechanisms — no storage layer duplication