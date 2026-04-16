---
id: ADR-002
title: openraft for Cluster Consensus
status: accepted
features:
- FT-002
- FT-014
- FT-092
- FT-095
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:873ce32d24849bc75a6317fb50a1852e620bd877f8688e531d06c1000f6cf3d1
---

**Status:** Accepted

**Context:** PiCloud requires distributed consensus for leader election, event log replication, and cluster membership. This is the foundational distributed systems problem. Using a proven library is strongly preferred over implementing Raft from scratch.

**Decision:** Use `openraft` for Raft consensus.

**Rationale:**
- Pure Rust implementation, no FFI, compiles cleanly to ARM64
- Actively maintained with a well-documented API
- Storage and network layers are pluggable — PiCloud can provide its own implementations
- Used in production systems (TiKV, among others)
- Supports both voter and learner roles, enabling flexible cluster size management

**Rejected alternatives:**
- **hashicorp/raft** — Go only. Ruled out by ADR-001.
- **Custom Raft implementation** — Raft is notoriously subtle. The cost of correctness is too high for a project that should be building on top of consensus, not debugging it.