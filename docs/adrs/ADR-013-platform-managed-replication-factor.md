---
id: ADR-013
title: Platform-Managed Replication Factor
status: accepted
features:
- FT-004
- FT-018
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:695e4f3ccd4601b3a2677bba1e2f9941f7c3456200c0b72bd1c53856650965dc
---

**Status:** Accepted

**Context:** Distributed storage systems typically allow operators to specify a replication factor per volume. This adds operational complexity and creates risk (under-replicated volumes, operator error).

**Decision:** The platform manages replication factor automatically based on cluster size. In MVP, all data uses full-replication (replicated to every node). Operators declare storage intent (durability tier), not replication factor.

**Rationale:**
- Eliminates a class of operator error (forgetting to set replication, setting it too low)
- Consistent with the abstraction model — operators declare intent, platform decides implementation
- On a 5-node Pi cluster, full-replication is feasible and NVMe bandwidth is sufficient
- Full-replication in MVP simplifies the storage implementation significantly

**Rejected alternatives:**
- **Operator-specified replication factor** — introduces a class of operator error (under-replicated volumes, inconsistent replication across the cluster) without meaningful benefit on a small Pi cluster.
- **No replication** — unacceptable for a platform that promises durability; a single node failure would mean data loss.

**Future:** Additional durability tiers (quorum, local) will be added in Phase 4 as the storage implementation matures.