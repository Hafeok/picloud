---
id: ADR-013
title: Platform-Managed Replication Factor
status: accepted
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Distributed storage systems typically allow operators to specify a replication factor per volume. This adds operational complexity and creates risk (under-replicated volumes, operator error).

**Decision:** The platform manages replication factor automatically based on cluster size. In MVP, all data uses full-replication (replicated to every node). Operators declare storage intent (durability tier), not replication factor.

**Rationale:**
- Eliminates a class of operator error (forgetting to set replication, setting it too low)
- Consistent with the abstraction model — operators declare intent, platform decides implementation
- On a 5-node Pi cluster, full-replication is feasible and NVMe bandwidth is sufficient
- Full-replication in MVP simplifies the storage implementation significantly

**Future:** Additional durability tiers (quorum, local) will be added in Phase 4 as the storage implementation matures.