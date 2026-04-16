---
id: ADR-057
title: Extended Storage Intent Tiers — Quorum, Local, and Performance Classes
status: proposed
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** ADR-024 established the storage intent model with a single durability tier (`full-replication`) for MVP. Production workloads have diverse storage needs — a database needs low-latency quorum-replicated storage, a build cache needs fast local-only storage, and a media archive needs sequential-write-optimized storage. A single tier forces all workloads to pay the replication cost of `full-replication`, which wastes NVMe capacity and I/O bandwidth on ephemeral or cache data.

**Decision:** Extend the storage intent model with additional durability tiers (`quorum`, `local`, `none`) and performance tiers (`fast`, `standard`, `archive`). Durability and performance are orthogonal dimensions — any combination is valid. The platform translates intent to implementation based on cluster state.

**Durability tiers:**
- `full-replication` — replicated to every node (unchanged from MVP)
- `quorum` — replicated to ⌈N/2⌉ + 1 nodes, using the same block replication infrastructure
- `local` — single node, pinned to the workload's scheduling node; does not follow workload migration
- `none` — tmpfs-backed, destroyed on container restart

**Performance tiers:**
- `standard` — balanced read/write (unchanged from MVP)
- `fast` — I/O scheduler priorities and placement preferences for low-latency random I/O
- `archive` — write-ahead batching and sequential layout for high-throughput sequential writes

**Tier immutability:** Changing a volume's durability or performance tier after creation is not supported. Operators create a new volume and migrate data. This avoids the complexity of online replication factor changes and data reorganization.

**Rationale:**
- Operators express requirements, not implementation — consistent with ADR-024's philosophy
- Orthogonal dimensions (durability × performance) give 12 combinations without a combinatorial explosion of named profiles
- `local` and `none` tiers enable workloads that would otherwise waste cluster replication capacity on disposable data
- Tier immutability keeps the storage subsystem simple and avoids risky online data reorganization

**Rejected alternatives:**
- **Named storage classes (bronze/silver/gold)** — hides the dimensions being controlled, making it impossible for operators to express "quorum durability but fast performance"
- **Online tier changes** — replication factor changes on a live volume risk data loss during transition and add significant complexity to the storage subsystem
- **Separate volume types per tier** — fragments the resource model unnecessarily when a single `volume` resource with intent fields is cleaner

**Consequences:**
- The scheduler must account for performance tier when placing volumes — `fast` prefers nodes with lower I/O utilization
- `local` volumes create a scheduling affinity between workload and node — the scheduler must be aware of this constraint
- Node drain (ADR-059) must handle `local` volumes explicitly — they cannot be migrated, only recreated