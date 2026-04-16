---
id: ADR-059
title: Node Drain and Graceful Workload Migration
status: proposed
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Operational maintenance — OS upgrades, hardware replacement, NVMe replacement — requires removing a node from the cluster temporarily. Without a drain mechanism, the operator must manually reschedule workloads, which is error-prone and risks downtime. The platform needs a first-class drain operation that safely migrates workloads before a node is taken offline.

**Decision:** Node drain is a platform-managed operation that cordons a node (prevents new workload scheduling), migrates all running workloads to surviving healthy nodes, and emits the full lifecycle as events with a shared correlation ID.

**Drain sequence:**
1. `NodeCordoned` — node is marked unschedulable in the RDF graph; no new workloads will be placed on it
2. `NodeDrainStarted` — platform begins migrating workloads
3. `WorkloadMigrated` × N — each workload is stopped on the drained node and started on a target node; each migration is recorded with source node, target node, workload IRI, and workload type
4. `NodeDrainCompleted` — all workloads have been migrated; the node has zero running workloads

**Target node selection:** Round-robin distribution across surviving healthy nodes, respecting resource constraints (ADR-058). If a workload's constraints cannot be satisfied on any surviving node, the drain fails with `DrainFailed` and the remaining workloads stay on the original node.

**Workload types:**
- **Containers** are stopped on the source node and started on the target node. Replicated volumes (`full-replication`, `quorum`) are already available on the target. `local` volumes are not migrated — the container starts with a fresh local volume.
- **Binaries** follow the same stop/start model.

**Cascading drain:** If a node that received migrated workloads is itself drained, those workloads migrate again to the remaining nodes. The migration log tracks the full chain.

**CLI:**
```bash
picloud cluster drain <node>     # drain a specific node
picloud cluster uncordon <node>  # mark node schedulable again
```

**Rationale:**
- Drain is the standard operational pattern for cluster maintenance — operators expect it
- Round-robin distribution avoids overloading a single target node
- Shared correlation ID across all drain events enables end-to-end tracing of the operation
- Explicit cordon/uncordon gives operators control over when a node re-enters the scheduling pool

**Rejected alternatives:**
- **Manual workload rescheduling** — error-prone, no audit trail, no atomic operation boundary
- **Automatic drain on node failure** — failure handling is a separate concern; drain is an intentional operator action, not a failure recovery mechanism
- **Live migration (memory state transfer)** — Pi 5 hardware and youki do not support live migration; stop/start is the only viable model

**Consequences:**
- The scheduler must respect the cordon flag when placing new workloads
- `local` volumes are not migrated — workloads using `local` storage must tolerate data loss on drain
- Drain operations are serialized — only one drain at a time to prevent cascading scheduling failures